//! Runs the whole server, apart from the network, through its HTTP routes.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use autorumble::config::Config;
use autorumble::engine::Engine;
use autorumble::time::now_ms;
use autorumble::web;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

const LIST: &str = "\
a.One 1.0,http://example.com/a.jar
b.Two 2.0,http://example.com/b.jar
c.Three 3.0,http://example.com/c.jar
";

struct TestServer {
    directory: TempDir,
    app: Router,
}

impl TestServer {
    fn start() -> TestServer {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("duel.txt"), LIST).unwrap();
        fs::write(directory.path().join("melee.txt"), LIST).unwrap();
        fs::create_dir(directory.path().join("logos")).unwrap();
        fs::create_dir(directory.path().join("robots")).unwrap();
        let app = serve(directory.path().to_path_buf());
        TestServer { directory, app }
    }

    /// Stops the server and starts a new one on the same files.
    fn restart(self) -> TestServer {
        let TestServer { directory, app } = self;
        drop(app);
        let app = serve(directory.path().to_path_buf());
        TestServer { directory, app }
    }

    async fn get(&self, path: &str) -> (StatusCode, String) {
        let request = Request::get(path).body(Body::empty()).unwrap();
        respond(&self.app, request).await
    }

    async fn post(&self, path: &str, body: String) -> (StatusCode, String) {
        let request = Request::post(path).body(Body::from(body)).unwrap();
        respond(&self.app, request).await
    }

    async fn upload(&self, line: String) -> String {
        let (status, text) = self.post("/UploadedResults", line + "\n").await;
        assert_eq!(status, StatusCode::OK);
        text
    }

    async fn rankings(&self, game: &str) -> Vec<Value> {
        let (status, text) = self.get(&format!("/Rankings?game={game}&api=1")).await;
        assert_eq!(status, StatusCode::OK);
        serde_json::from_str::<Value>(&text).unwrap().as_array().unwrap().clone()
    }

    fn write_list(&self, text: &str) {
        fs::write(self.directory.path().join("duel.txt"), text).unwrap();
    }
}

/// Reads the lists on every upload and recomputes the scores straight away.
fn serve(directory: PathBuf) -> Router {
    let path = |name: &str| directory.join(name).display().to_string();
    let config = Config::parse(&format!(
        r#"
        listen = "127.0.0.1:0"
        database = "{database}"
        logos = "{logos}"
        logo_list = "{logo_list}"
        robots = "{robots}"
        standings_seconds = 0
        participants_seconds = 0
        allowed_clients = ["1.11.2"]

        [[rumble]]
        name = "duel"
        title = "Duel"
        field = "800x600"
        rounds = 35
        hide_names = true
        participants = {{ file = "{duel}" }}

        [[rumble]]
        name = "melee"
        title = "Melee"
        field = "800x800"
        rounds = 35
        melee_bots = 6
        participants = {{ file = "{melee}" }}
        "#,
        database = path("test.db"),
        logos = path("logos"),
        logo_list = path("logos.csv"),
        robots = path("robots"),
        duel = path("duel.txt"),
        melee = path("melee.txt"),
    ))
    .unwrap();
    web::router(Arc::new(Engine::start(config).unwrap()))
}

async fn respond(app: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// A result as the client sends it. The time tells results of the same robots apart.
fn result(first: &str, first_score: i64, second: &str, second_score: i64, time_ms: i64) -> String {
    format!(
        "game=duel&version=1&client=1.11.2&teams=NOT&melee=NOT&rounds=35&field=800x600&user=Tester\
         &time={time_ms}&fname={first}&fscore={first_score}&fbulletd=100&fsurvival=20\
         &sname={second}&sscore={second_score}&sbulletd=100&ssurvival=15&hidenames=YES"
    )
}

fn close(value: &Value, expected: f64) -> bool {
    (value.as_f64().unwrap() - expected).abs() < 1e-9
}

#[tokio::test]
async fn a_result_shows_up_in_the_rankings() {
    let server = TestServer::start();
    let reply = server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, now_ms())).await;
    assert!(reply.contains("OK"), "{reply}");

    let rankings = server.rankings("duel").await;
    assert_eq!(rankings.len(), 3);
    assert_eq!(rankings[0]["name"], "a.One 1.0");
    assert!(close(&rankings[0]["APS"], 75.0));
    assert_eq!(rankings[0]["pairings"], 1);
    assert_eq!(rankings[1]["name"], "b.Two 2.0");
    assert!(close(&rankings[1]["APS"], 25.0));
    assert_eq!(rankings[2]["name"], "c.Three 3.0");
    assert_eq!(rankings[2]["battles"], 0);
}

#[tokio::test]
async fn results_count_the_same_whichever_robot_comes_first() {
    let server = TestServer::start();
    let time = now_ms();
    server.upload(result("b.Two 2.0", 100, "a.One 1.0", 300, time)).await;
    let rankings = server.rankings("duel").await;
    assert_eq!(rankings[0]["name"], "a.One 1.0");
    assert!(close(&rankings[0]["APS"], 75.0));
}

#[tokio::test]
async fn sending_a_result_twice_counts_it_once() {
    let server = TestServer::start();
    let line = result("a.One 1.0", 300, "b.Two 2.0", 100, now_ms());
    server.upload(line.clone()).await;
    let reply = server.upload(line).await;
    assert!(reply.contains("OK. Duplicate"), "{reply}");
    assert_eq!(server.rankings("duel").await[0]["battles"], 1);
}

#[tokio::test]
async fn rejections_tell_the_stock_client_not_to_resend() {
    let server = TestServer::start();
    let time = now_ms();
    let unlisted = server.upload(result("a.One 1.0", 300, "x.Other 1", 100, time)).await;
    assert!(unlisted.contains("OK. ERROR"), "{unlisted}");
    assert!(unlisted.contains("not a participant"), "{unlisted}");

    let names_shown =
        result("a.One 1.0", 300, "b.Two 2.0", 100, time).replace("&hidenames=YES", "");
    let reply = server.upload(names_shown).await;
    assert!(reply.contains("HIDENAMES=YES"), "{reply}");

    let data_cleared = result("a.One 1.0", 300, "b.Two 2.0", 100, time) + "&cleardata=YES";
    let reply = server.upload(data_cleared).await;
    assert!(reply.contains("CLEARDATA=NOT"), "{reply}");

    let reply = server
        .upload(result("a.One 1.0", 1, "b.Two 2.0", 1, time).replace("game=duel", "game=typo"))
        .await;
    assert!(reply.contains("there is no rumble typo"), "{reply}");

    let reply = server.upload("complete nonsense".to_string()).await;
    assert!(reply.contains("OK. ERROR"), "{reply}");

    assert_eq!(server.rankings("duel").await[0]["battles"], 0);
}

#[tokio::test]
async fn a_batch_answers_each_line_in_order() {
    let server = TestServer::start();
    let time = now_ms();
    let body = [
        result("a.One 1.0", 300, "b.Two 2.0", 100, time),
        result("a.One 1.0", 300, "x.Other 1", 100, time),
        result("a.One 1.0", 300, "b.Two 2.0", 100, time),
        result("a.One 1.0", 300, "c.Three 3.0", 100, time),
    ]
    .join("\n");
    let (status, text) = server.post("/UploadedResultsBatch", body).await;
    assert_eq!(status, StatusCode::OK);

    let lines: Vec<&str> = text.lines().filter(|line| !line.starts_with('[')).collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert_eq!(lines[0], "OK stored");
    assert!(lines[1].starts_with("REJECT"), "{text}");
    assert_eq!(lines[2], "OK duplicate");
    assert_eq!(lines[3], "OK stored");

    assert_eq!(server.rankings("duel").await[0]["battles"], 2);
}

#[tokio::test]
async fn an_oversized_batch_is_refused() {
    let server = TestServer::start();
    let body = vec!["line"; web::MAX_BATCH_LINES + 1].join("\n");
    let (status, _) = server.post("/UploadedResultsBatch", body).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn priority_battles_name_listed_robots_with_underscores() {
    let server = TestServer::start();
    let reply = server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, now_ms())).await;
    let line = reply.lines().find(|line| line.starts_with('[')).expect(&reply);
    // The computed queue held the pairings that were missing before this result.
    let pair = line.trim_matches(['[', ']']);
    let robots: Vec<&str> = pair.split(',').collect();
    assert_eq!(robots.len(), 2);
    for robot in robots {
        assert!(["a.One_1.0", "b.Two_2.0", "c.Three_3.0"].contains(&robot), "{robot}");
    }
}

#[tokio::test]
async fn scores_survive_a_restart() {
    let server = TestServer::start();
    let time = now_ms();
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time)).await;
    server.upload(result("a.One 1.0", 100, "c.Three 3.0", 300, time)).await;
    server.upload(result("b.Two 2.0", 123, "c.Three 3.0", 321, time + 1)).await;
    let before = server.rankings("duel").await;

    let server = server.restart();
    assert_eq!(server.rankings("duel").await, before);

    let reply = server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time)).await;
    assert!(reply.contains("Duplicate"), "{reply}");
}

#[tokio::test]
async fn the_list_decides_who_is_in_the_rumble() {
    let server = TestServer::start();
    let time = now_ms();
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time)).await;
    server.upload(result("a.One 1.0", 300, "c.Three 3.0", 100, time)).await;

    // Retiring a robot takes it out of the rankings and out of the other robots' scores.
    server.write_list("a.One 1.0,http://x\nc.Three 3.0,http://x\n");
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time + 1)).await;
    let rankings = server.rankings("duel").await;
    assert_eq!(rankings.len(), 2);
    assert_eq!(rankings[0]["pairings"], 1);

    // Bringing it back restores its results.
    server.write_list(LIST);
    server.upload(result("a.One 1.0", 300, "c.Three 3.0", 100, time + 2)).await;
    let rankings = server.rankings("duel").await;
    assert_eq!(rankings.len(), 3);
    assert_eq!(rankings[0]["pairings"], 2);
}

#[tokio::test]
async fn a_bad_fetch_never_retires_anyone() {
    let server = TestServer::start();
    let time = now_ms();
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time)).await;

    server.write_list("");
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time + 1)).await;
    assert_eq!(server.rankings("duel").await.len(), 3);

    fs::remove_file(server.directory.path().join("duel.txt")).unwrap();
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time + 2)).await;
    assert_eq!(server.rankings("duel").await.len(), 3);
}

#[tokio::test]
async fn a_client_cannot_retire_a_robot() {
    let server = TestServer::start();
    let (status, text) = server
        .post("/RemoveOldParticipant", "version=1&game=duel&name=a.One_1.0&dummy=NA".to_string())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("OK"));
    assert_eq!(server.rankings("duel").await.len(), 3);
}

#[tokio::test]
async fn clients_can_download_what_they_need() {
    let server = TestServer::start();
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, now_ms())).await;

    let (_, list) = server.get("/Participants?game=duel").await;
    assert!(list.starts_with("<pre>\n"), "{list}");
    assert!(list.contains("a.One 1.0,http://example.com/a.jar\n"), "{list}");
    assert!(list.ends_with("</pre>\n"), "{list}");

    let (_, ratings) = server.get("/RatingsFile?version=1&game=duel").await;
    assert!(ratings.contains("a.One_1.0=75,1,"), "{ratings}");
    assert!(!ratings.contains("c.Three"), "only robots with battles: {ratings}");

    let (status, _) = server.get("/RatingsFile?game=duel").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = server.get("/RatingsFile?version=1&game=typo").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_pages_render() {
    let server = TestServer::start();
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, now_ms())).await;
    server.upload(result("a.One 1.0", 300, "c.Three 3.0", 100, now_ms())).await;
    server.upload(result("b.Two 2.0", 300, "c.Three 3.0", 100, now_ms())).await;

    let pages = [
        ("/", "Duel"),
        ("/Rankings?game=duel", "a.One 1.0"),
        ("/Rankings?game=duel&order=-Vote&limit=1", "c.Three 3.0"),
        ("/BotDetails?game=duel&name=a.One%201.0", "b.Two 2.0"),
        ("/BotDetails?game=duel&name=a.One%201.0", "BattleDetails?game=duel"),
        ("/BotCompare?game=duel&bota=a.One%201.0&botb=b.Two%202.0", "c.Three 3.0"),
        (
            "/BattleDetails?game=duel&name=a.One%201.0&opponent=b.Two%202.0",
            "a.One 1.0 against b.Two 2.0",
        ),
        ("/RumbleStats", "Tester"),
        ("/ScoreExplanation", "APS"),
        ("/robots.txt", "BotCompare"),
        ("/style.css", "body"),
    ];
    for (path, expected) in pages {
        let (status, text) = server.get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(text.contains(expected), "{path}: {text}");
    }

    let (status, _) = server.get("/BotDetails?game=duel&name=nobody").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, details) = server.get("/BotDetails?game=duel&name=a.One%201.0&api=1").await;
    let details: Value = serde_json::from_str(&details).unwrap();
    assert_eq!(details["pairingsList"].as_array().unwrap().len(), 2);
    assert!(close(&details["ANPP"], 100.0));
}

#[tokio::test]
async fn a_pairing_shows_each_of_its_battles() {
    let server = TestServer::start();
    let time = now_ms();
    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, time)).await;
    server.upload(result("b.Two 2.0", 300, "a.One 1.0", 100, time + 1)).await;
    server.upload(result("a.One 1.0", 300, "c.Three 3.0", 100, time + 2)).await;

    // From each robot's side, newest first, with its own share of the two scores.
    let (status, page) =
        server.get("/BattleDetails?game=duel&name=a.One%201.0&opponent=b.Two%202.0").await;
    assert_eq!(status, StatusCode::OK);
    let aps = |page: &str, value: &str| page.find(&format!(">{value}</td>")).expect(value);
    assert!(aps(&page, "25.00") < aps(&page, "75.00"), "{page}");
    assert!(page.contains("class=\"bad\">25.00"), "{page}");
    assert!(page.contains("class=\"good\">75.00"), "{page}");
    assert!(page.contains("Tester"), "{page}");
    assert!(!page.contains("Show all"), "{page}");

    let (_, oldest_first) = server
        .get("/BattleDetails?game=duel&name=b.Two%202.0&opponent=a.One%201.0&order=-Date")
        .await;
    assert!(aps(&oldest_first, "25.00") < aps(&oldest_first, "75.00"), "{oldest_first}");

    // A limit keeps the newest and offers the rest.
    let (_, limited) =
        server.get("/BattleDetails?game=duel&name=a.One%201.0&opponent=b.Two%202.0&limit=1").await;
    assert!(limited.contains(">25.00</td>"), "{limited}");
    assert!(limited.contains("The newest 1 of 2 battles"), "{limited}");
    assert!(limited.contains("limit=2"), "{limited}");

    let (status, _) =
        server.get("/BattleDetails?game=duel&name=b.Two%202.0&opponent=c.Three%203.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "they have not fought");
    let (status, _) = server.get("/BattleDetails?game=duel&name=a.One%201.0&opponent=nobody").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) =
        server.get("/BattleDetails?game=melee&name=a.One%201.0&opponent=b.Two%202.0").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "they have only fought in the other rumble");
}

#[tokio::test]
async fn uploader_names_are_escaped() {
    let server = TestServer::start();
    let line = result("a.One 1.0", 300, "b.Two 2.0", 100, now_ms())
        .replace("user=Tester", "user=<script>alert(1)</script>");
    server.upload(line).await;
    let (_, stats) = server.get("/RumbleStats").await;
    assert!(!stats.contains("<script>"), "{stats}");
    assert!(stats.contains("&lt;script&gt;"), "{stats}");
}

#[tokio::test]
async fn logos_are_served_when_valid() {
    let server = TestServer::start();
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    png.extend(b"IHDR");
    png.extend(16_u32.to_be_bytes());
    png.extend(16_u32.to_be_bytes());
    let logos = server.directory.path().join("logos");
    fs::write(logos.join("flag_usa.png"), &png).unwrap();
    fs::write(logos.join("bad.png"), b"<svg></svg> not a png at all, just text").unwrap();
    fs::write(server.directory.path().join("logos.csv"), "A,Flag_USA\nb,bad\n").unwrap();

    server.upload(result("a.One 1.0", 300, "b.Two 2.0", 100, now_ms())).await;

    let request = Request::get("/logos/flag_usa.png").body(Body::empty()).unwrap();
    let response = server.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(response.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");

    let (status, _) = server.get("/logos/bad.png").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = server.get("/logos/..%2Fsecret.png").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let rankings = server.rankings("duel").await;
    assert_eq!(rankings[0]["logo"], "/logos/flag_usa.png");
    assert_eq!(rankings[1]["logo"], Value::Null);

    let (_, page) = server.get("/BotDetails?game=duel&name=a.One%201.0").await;
    assert!(page.contains("src=\"/logos/flag_usa.png\""), "{page}");
}

#[tokio::test]
async fn robot_jars_are_served_and_nothing_else() {
    let server = TestServer::start();
    let robots = server.directory.path().join("robots");
    fs::write(robots.join("a.One_1.0.jar"), b"jar bytes").unwrap();
    fs::write(robots.join("notes.txt"), b"not a jar").unwrap();
    fs::write(server.directory.path().join("secret.jar"), b"outside").unwrap();

    let request = Request::get("/robots/a.One_1.0.jar").body(Body::empty()).unwrap();
    let response = server.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/java-archive");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"jar bytes");

    for path in [
        "/robots/missing.jar",
        "/robots/notes.txt",
        "/robots/..%2Fsecret.jar",
        "/robots/.hidden.jar",
    ] {
        let (status, _) = server.get(path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}
