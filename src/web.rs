//! The HTTP routes. Upload handlers pass the text on to the engine, and everything else reads the
//! latest snapshot. `docs/protocol.md` describes what the Robocode client uses.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};

use crate::engine::{Engine, RumbleView, Snapshot};
use crate::pages::{self, Order};
use crate::participants;
use crate::protocol;
use crate::time;

/// The largest request body, which is plenty for [`MAX_BATCH_LINES`] results.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// The most results in one batch.
pub const MAX_BATCH_LINES: usize = 1000;

type Params = Query<HashMap<String, String>>;

/// Builds the web application around a running engine.
pub fn router(engine: Arc<Engine>) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/Rankings", get(rankings))
        .route("/BotDetails", get(bot_details))
        .route("/BotCompare", get(bot_compare))
        .route("/BattleDetails", get(battle_details))
        .route("/RumbleStats", get(rumble_stats))
        .route("/ScoreExplanation", get(|| async { Html(pages::score_explanation()) }))
        .route("/RatingsFile", get(ratings_file))
        .route("/Participants", get(participants_list))
        .route("/UploadedResults", post(uploaded_results))
        .route("/UploadedResultsBatch", post(uploaded_results_batch))
        .route("/RemoveOldParticipant", get(remove_old_participant).post(remove_old_participant))
        .route("/logos/{file}", get(logo))
        .route("/robots/{file}", get(robot_jar))
        .route("/style.css", get(style))
        .route("/robots.txt", get(|| async { text(pages::ROBOTS_TXT.to_string()) }))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(engine)
}

fn text(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

/// A request for something that does not exist, with the message for the client.
struct NotFound(&'static str);

impl IntoResponse for NotFound {
    fn into_response(self) -> Response {
        (StatusCode::NOT_FOUND, self.0).into_response()
    }
}

/// Finds the rumble named by the `game` parameter in the snapshot.
fn find_rumble<'a>(
    snapshot: &'a Snapshot,
    params: &HashMap<String, String>,
) -> Result<&'a RumbleView, NotFound> {
    let game = params.get("game").map_or("", String::as_str);
    snapshot.rumble(game).ok_or(NotFound("There is no such rumble."))
}

/// Finds a robot of a rumble by the value of a parameter.
fn find_bot(
    rumble: &RumbleView,
    params: &HashMap<String, String>,
    key: &str,
) -> Result<usize, NotFound> {
    let name = params.get(key).map_or("", String::as_str);
    let index = rumble.standings.by_name.get(name);
    index.copied().ok_or(NotFound("There is no such robot in this rumble."))
}

/// A limit parameter, or no limit.
fn limit(params: &HashMap<String, String>) -> usize {
    params.get("limit").and_then(|limit| limit.parse().ok()).unwrap_or(usize::MAX)
}

fn wants_json(params: &HashMap<String, String>) -> bool {
    params.contains_key("api")
}

async fn home(State(engine): State<Arc<Engine>>) -> Html<String> {
    Html(pages::home(&engine.snapshot()))
}

async fn rankings(
    State(engine): State<Arc<Engine>>,
    Query(params): Params,
) -> Result<Response, NotFound> {
    let snapshot = engine.snapshot();
    let rumble = find_rumble(&snapshot, &params)?;
    let order = Order::parse(params.get("order"), "APS");
    Ok(if wants_json(&params) {
        Json(pages::rankings_json(rumble, &snapshot.logos, &order, limit(&params))).into_response()
    } else {
        Html(pages::rankings(rumble, &snapshot.logos, &order, limit(&params))).into_response()
    })
}

async fn bot_details(
    State(engine): State<Arc<Engine>>,
    Query(params): Params,
) -> Result<Response, NotFound> {
    let snapshot = engine.snapshot();
    let rumble = find_rumble(&snapshot, &params)?;
    let bot = find_bot(rumble, &params, "name")?;
    let order = Order::parse(params.get("order"), "APS");
    Ok(if wants_json(&params) {
        Json(pages::bot_details_json(rumble, bot, &snapshot.logos, &order)).into_response()
    } else {
        Html(pages::bot_details(rumble, bot, &snapshot.logos, &order)).into_response()
    })
}

async fn bot_compare(
    State(engine): State<Arc<Engine>>,
    Query(params): Params,
) -> Result<Html<String>, NotFound> {
    let snapshot = engine.snapshot();
    let rumble = find_rumble(&snapshot, &params)?;
    let bot_a = find_bot(rumble, &params, "bota")?;
    let bot_b = find_bot(rumble, &params, "botb")?;
    let order = Order::parse(params.get("order"), "Diff APS");
    Ok(Html(pages::bot_compare(rumble, bot_a, bot_b, &order)))
}

/// Every battle of one pairing, newest first. A pairing can have many battles, so this shows the
/// newest ones unless `limit` asks for more.
async fn battle_details(
    State(engine): State<Arc<Engine>>,
    Query(params): Params,
) -> Result<Response, NotFound> {
    let snapshot = engine.snapshot();
    let rumble = find_rumble(&snapshot, &params)?;
    let bot = find_bot(rumble, &params, "name")?;
    let opponent = find_bot(rumble, &params, "opponent")?;
    let standings = &rumble.standings;
    let pairing = standings.pairings[bot]
        .iter()
        .find(|pairing| pairing.opponent == opponent)
        .ok_or(NotFound("These robots have not fought yet."))?;
    let shown = params.get("limit").and_then(|limit| limit.parse().ok());
    let battles = engine
        .battles_between(
            &rumble.config.name,
            &standings.bots[bot].name,
            &standings.bots[opponent].name,
            shown.unwrap_or(pages::BATTLES_SHOWN),
        )
        .await;
    Ok(match battles {
        Ok(battles) => {
            let order = Order::parse(params.get("order"), "Date");
            Html(pages::battle_details(
                rumble,
                bot,
                pairing,
                &battles,
                shown,
                &snapshot.logos,
                &order,
            ))
            .into_response()
        }
        Err(reason) => {
            eprintln!("web: cannot read battles: {reason}");
            (StatusCode::INTERNAL_SERVER_ERROR, "The battles cannot be read now.").into_response()
        }
    })
}

async fn rumble_stats(State(engine): State<Arc<Engine>>) -> Html<String> {
    Html(pages::rumble_stats(&engine.snapshot()))
}

/// The ratings the client downloads: `name_version=APS,battles,date` for each robot with battles.
async fn ratings_file(
    State(engine): State<Arc<Engine>>,
    Query(params): Params,
) -> Result<Response, NotFound> {
    if params.get("version").map(String::as_str) != Some("1") {
        return Err(NotFound("The version must be 1."));
    }
    let snapshot = engine.snapshot();
    let rumble = find_rumble(&snapshot, &params)?;
    let mut ratings = String::new();
    for bot in rumble.standings.bots.iter().filter(|bot| bot.battles > 0) {
        ratings.push_str(&format!(
            "{}={},{},{}
",
            bot.name.replace(' ', "_"),
            bot.aps,
            bot.battles,
            time::format_ms(bot.last_ms)
        ));
    }
    Ok(text(ratings))
}

/// The participant list in the form the client downloads, wherever the server got it from.
async fn participants_list(
    State(engine): State<Arc<Engine>>,
    Query(params): Params,
) -> Result<Response, NotFound> {
    let snapshot = engine.snapshot();
    let rumble = find_rumble(&snapshot, &params)?;
    Ok(text(participants::to_client_text(&rumble.participants)))
}

/// One result from a client without batch support.
async fn uploaded_results(State(engine): State<Arc<Engine>>, body: Bytes) -> Response {
    let body = String::from_utf8_lossy(&body);
    let line = body.lines().next().unwrap_or_default().to_string();
    let reply = engine.upload(vec![line]).await;
    text(protocol::single_reply(&reply.outcomes[0], &reply.priority))
}

/// Many results in one request, one per line.
async fn uploaded_results_batch(State(engine): State<Arc<Engine>>, body: Bytes) -> Response {
    let body = String::from_utf8_lossy(&body);
    let lines: Vec<String> =
        body.lines().filter(|line| !line.trim().is_empty()).map(str::to_string).collect();
    if lines.len() > MAX_BATCH_LINES {
        let message = format!("A batch can have at most {MAX_BATCH_LINES} results.");
        return (StatusCode::PAYLOAD_TOO_LARGE, message).into_response();
    }
    let reply = engine.upload(lines).await;
    text(protocol::batch_reply(&reply.outcomes, &reply.priority))
}

/// Clients ask for robots they no longer see on the list to be retired. The server decides this
/// from the participant list itself, so a client with an old list cannot retire anyone.
async fn remove_old_participant() -> Response {
    text("OK. Participants are managed by the server.\n".to_string())
}

async fn logo(
    State(engine): State<Arc<Engine>>,
    Path(file): Path<String>,
) -> Result<Response, NotFound> {
    let snapshot = engine.snapshot();
    let bytes = file.strip_suffix(".png").and_then(|name| snapshot.logos.image(name));
    let bytes = bytes.ok_or(NotFound("There is no such image."))?;
    let headers = [
        (header::CONTENT_TYPE, "image/png"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::CACHE_CONTROL, "public, max-age=86400"),
    ];
    Ok((headers, bytes.as_ref().clone()).into_response())
}

/// A robot's jar, for robots that are not hosted anywhere else. Only plain file names ending in
/// `.jar` are accepted, so a request cannot reach anything outside the directory.
async fn robot_jar(
    State(engine): State<Arc<Engine>>,
    Path(file): Path<String>,
) -> Result<Response, NotFound> {
    const MISSING: NotFound = NotFound("There is no such robot jar.");
    let plain = file.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    if !plain || file.starts_with('.') || !file.ends_with(".jar") {
        return Err(MISSING);
    }
    let directory = engine.robots_directory().ok_or(MISSING)?;
    let bytes = tokio::fs::read(directory.join(&file)).await.map_err(|_| MISSING)?;
    Ok(([(header::CONTENT_TYPE, "application/java-archive")], bytes).into_response())
}

async fn style() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], pages::STYLE).into_response()
}
