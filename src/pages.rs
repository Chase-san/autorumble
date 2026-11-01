//! Renders the pages from a snapshot. Plain HTML with no scripts: columns sort through links, with
//! `order=APS` for highest first and `order=-APS` for lowest first.
//!
//! Adding `api=1` to the Rankings or BotDetails page gives the same data as JSON.

use std::cmp::Ordering;

use serde_json::{Value, json};

use crate::engine::{RumbleView, Snapshot};
use crate::logos::Logos;
use crate::scoring::{BotScore, PairingView, battle_aps};
use crate::store::BattleSide;
use crate::time::format_ms;

pub const STYLE: &str = include_str!("style.css");
pub const ROBOTS_TXT: &str = "User-agent: *\nDisallow: /BotCompare\nDisallow: /BattleDetails\n";

/// How many battles of a pairing to show unless the page is asked for more.
pub const BATTLES_SHOWN: usize = 1000;

/// The limits of a score: below the first it is bad, and above the second it is good. The same
/// numbers as LiteRumble.
type Limits = (f64, f64);
const APS_LIMITS: Limits = (40.0, 60.0);
const NPP_LIMITS: Limits = (40.0, 70.0);
const SURVIVAL_LIMITS: Limits = (40.0, 60.0);
const KNN_PBI_LIMITS: Limits = (-0.1, 0.1);

/// The column to sort by, and the direction.
pub struct Order {
    pub column: String,
    pub ascending: bool,
}

impl Order {
    pub fn parse(param: Option<&String>, default: &str) -> Order {
        let value = param.map_or(default, String::as_str);
        match value.strip_prefix('-') {
            Some(column) => Order { column: column.to_string(), ascending: true },
            None => Order { column: value.to_string(), ascending: false },
        }
    }
}

/// A cell value that a column sorts by.
enum Key {
    Number(f64),
    Text(String),
}

/// Sorts rows by the key of the chosen column. The sort is stable, so rows with the same key, and
/// all rows for an unknown column, keep their order.
fn sort_rows<T>(rows: &mut [T], order: &Order, key: impl Fn(&T, &str) -> Option<Key>) {
    rows.sort_by(|a, b| {
        let ordering = match (key(a, &order.column), key(b, &order.column)) {
            (Some(Key::Number(a)), Some(Key::Number(b))) => a.total_cmp(&b),
            (Some(Key::Text(a)), Some(Key::Text(b))) => a.cmp(&b),
            _ => Ordering::Equal,
        };
        if order.ascending { ordering } else { ordering.reverse() }
    });
}

/// Escapes text for HTML, in content and in attribute values.
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Encodes text for a URL query value.
pub fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// A complete page.
fn page(title: &str, body: &str) -> String {
    let title = escape(title);
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n<link rel=\"stylesheet\" href=\"/style.css\">\n</head>\n<body>\n\
         <h1>{title}</h1>\n<nav><a href=\"/\">Home</a> <a href=\"/RumbleStats\">Statistics</a> \
         <a href=\"/ScoreExplanation\">Score explanation</a></nav>\n{body}\n</body>\n</html>\n"
    )
}

/// The heading row of a table, where each column is a link that sorts by it.
fn sort_header(base: &str, columns: &[&str], order: &Order) -> String {
    let mut row = String::from("<tr>");
    for column in columns {
        if column.is_empty() {
            row.push_str("<th></th>");
            continue;
        }
        let sorted = order.column == *column;
        let target =
            if sorted && !order.ascending { format!("-{column}") } else { column.to_string() };
        let class = if sorted { " class=\"sortedby\"" } else { "" };
        row.push_str(&format!(
            "<th{class}><a href=\"{base}&amp;order={}\">{}</a></th>",
            encode(&target),
            escape(column)
        ));
    }
    row.push_str("</tr>\n");
    row
}

fn logo_tag(logos: &Logos, name: &str) -> String {
    match logos.of_robot(name) {
        Some(image) => {
            format!("<img class=\"logo\" src=\"/logos/{}.png\" alt=\"\">", encode(image))
        }
        None => String::new(),
    }
}

fn logo_path(logos: &Logos, name: &str) -> Value {
    match logos.of_robot(name) {
        Some(image) => json!(format!("/logos/{image}.png")),
        None => Value::Null,
    }
}

fn details_href(game: &str, name: &str) -> String {
    format!("BotDetails?game={}&amp;name={}", encode(game), encode(name))
}

fn battles_href(game: &str, name: &str, opponent: &str) -> String {
    format!(
        "BattleDetails?game={}&amp;name={}&amp;opponent={}",
        encode(game),
        encode(name),
        encode(opponent)
    )
}

fn cell(content: &str) -> String {
    format!("<td>{content}</td>")
}

fn number(value: f64) -> String {
    cell(&format!("{value:.2}"))
}

/// A score's cell, coloured when the score is good or bad.
fn rated(value: f64, limits: Limits) -> String {
    let (bad_below, good_above) = limits;
    let class = if value < bad_below {
        " class=\"bad\""
    } else if value > good_above {
        " class=\"good\""
    } else {
        ""
    };
    format!("<td{class}>{value:.2}</td>")
}

fn ci_text(ci: f64) -> String {
    if ci < 0.0 { cell("n/a") } else { cell(&format!("&plusmn;{ci:.2}")) }
}

/// A table with one row of values under plain headings.
fn summary_table(headings: &[&str], cells: &[String]) -> String {
    let headings: String = headings.iter().map(|heading| format!("<th>{heading}</th>")).collect();
    format!("<table>\n<tr>{headings}</tr>\n<tr>{}</tr>\n</table>\n", cells.concat())
}

/// A robot's logo and name as a link to its page.
fn robot_link(logos: &Logos, game: &str, name: &str) -> String {
    format!(
        "{} <a href=\"{}\">{}</a>",
        logo_tag(logos, name),
        details_href(game, name),
        escape(name)
    )
}

/// The front page: every rumble, by kind.
pub fn home(snapshot: &Snapshot) -> String {
    let mut body = String::new();
    for (heading, melee) in [("1v1", false), ("Melee", true)] {
        let rumbles = snapshot.rumbles.iter().filter(|rumble| rumble.config.is_melee() == melee);
        let mut rows = String::new();
        for rumble in rumbles {
            let game = encode(&rumble.config.name);
            rows.push_str(&format!(
                "<tr><td><a href=\"Rankings?game={game}\">{}</a></td><td>{}</td><td>{} robots per battle on {}</td><td>{}</td></tr>\n",
                escape(&rumble.config.title),
                rumble.standings.bots.len(),
                rumble.config.melee_bots.unwrap_or(2),
                escape(&rumble.config.field),
                rumble.total_battles,
            ));
        }
        if !rows.is_empty() {
            body.push_str(&format!(
                "<h2>{heading}</h2>\n<table>\n<tr><th>Rumble</th><th>Participants</th><th>Format</th><th>Results</th></tr>\n{rows}</table>\n"
            ));
        }
    }
    page("AutoRumble", &body)
}

fn sorted_ranking<'a>(rumble: &'a RumbleView, order: &Order) -> Vec<&'a BotScore> {
    let mut bots: Vec<&BotScore> = rumble.standings.bots.iter().collect();
    sort_rows(&mut bots, order, |bot, column| {
        Some(match column {
            "Competitor" => Key::Text(bot.name.clone()),
            "APS" => Key::Number(bot.aps),
            "APS CI" => Key::Number(bot.aps_ci),
            "PWIN" => Key::Number(bot.pwin),
            "ANPP" => Key::Number(bot.anpp),
            "Vote" => Key::Number(bot.vote),
            "Survival" => Key::Number(bot.survival),
            "Pairings" => Key::Number(f64::from(bot.pairings)),
            "Battles" => Key::Number(bot.battles as f64),
            "Latest Battle" => Key::Number(bot.last_ms as f64),
            _ => return None,
        })
    });
    bots
}

/// The rankings of a rumble.
pub fn rankings(rumble: &RumbleView, logos: &Logos, order: &Order, limit: usize) -> String {
    let game = &rumble.config.name;
    let columns = [
        "",
        "",
        "Competitor",
        "APS",
        "APS CI",
        "PWIN",
        "ANPP",
        "Vote",
        "Survival",
        "Pairings",
        "Battles",
        "Latest Battle",
    ];
    let base = format!("Rankings?game={}", encode(game));
    let mut rows = String::new();
    for (rank, bot) in sorted_ranking(rumble, order).into_iter().take(limit).enumerate() {
        rows.push_str("<tr>");
        rows.push_str(&cell(&(rank + 1).to_string()));
        rows.push_str(&cell(&logo_tag(logos, &bot.name)));
        rows.push_str(&cell(&format!(
            "<a href=\"{}\">{}</a>",
            details_href(game, &bot.name),
            escape(&bot.name)
        )));
        rows.push_str(&number(bot.aps));
        rows.push_str(&ci_text(bot.aps_ci));
        rows.push_str(&number(bot.pwin));
        rows.push_str(&number(bot.anpp));
        rows.push_str(&number(bot.vote));
        rows.push_str(&number(bot.survival));
        rows.push_str(&cell(&bot.pairings.to_string()));
        rows.push_str(&cell(&bot.battles.to_string()));
        rows.push_str(&cell(&format_ms(bot.last_ms)));
        rows.push_str("</tr>\n");
    }
    let stability = if rumble.standings.stable { "Rankings stable" } else { "Rankings not stable" };
    let body = format!(
        "<p>{} robots. {stability}.</p>\n<table>\n{}{rows}</table>\n",
        rumble.standings.bots.len(),
        sort_header(&base, &columns, order),
    );
    page(&format!("{} rankings", rumble.config.title), &body)
}

/// The rankings as JSON.
pub fn rankings_json(rumble: &RumbleView, logos: &Logos, order: &Order, limit: usize) -> Value {
    let rows: Vec<Value> = sorted_ranking(rumble, order)
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(rank, bot)| {
            json!({
                "name": bot.name,
                "logo": logo_path(logos, &bot.name),
                "rank": rank + 1,
                "APS": bot.aps,
                "APS_CI": bot.aps_ci,
                "PWIN": bot.pwin,
                "ANPP": bot.anpp,
                "vote": bot.vote,
                "survival": bot.survival,
                "pairings": bot.pairings,
                "battles": bot.battles,
                "latest": format_ms(bot.last_ms),
            })
        })
        .collect();
    Value::Array(rows)
}

fn sorted_pairings<'a>(rumble: &'a RumbleView, bot: usize, order: &Order) -> Vec<&'a PairingView> {
    let standings = &rumble.standings;
    let mut pairings: Vec<&PairingView> = standings.pairings[bot].iter().collect();
    sort_rows(&mut pairings, order, |pair, column| {
        Some(match column {
            "Competitor" => Key::Text(standings.bots[pair.opponent].name.clone()),
            "APS" => Key::Number(pair.aps),
            "APS CI" => Key::Number(pair.aps_ci),
            "Min APS" => Key::Number(pair.min_aps),
            "NPP" => Key::Number(pair.npp),
            "Survival" => Key::Number(pair.survival),
            "KNNPBI" => Key::Number(pair.knn_pbi),
            "Battles" => Key::Number(f64::from(pair.battles)),
            "Latest Battle" => Key::Number(pair.last_ms as f64),
            _ => return None,
        })
    });
    pairings
}

/// One robot's scores and pairings.
pub fn bot_details(rumble: &RumbleView, bot: usize, logos: &Logos, order: &Order) -> String {
    let game = &rumble.config.name;
    let score = &rumble.standings.bots[bot];
    let columns = [
        "",
        "",
        "Competitor",
        "APS",
        "APS CI",
        "Min APS",
        "NPP",
        "Survival",
        "KNNPBI",
        "Battles",
        "Latest Battle",
        "",
        "",
    ];
    let base = details_href(game, &score.name);
    let mut rows = String::new();
    for (rank, pair) in sorted_pairings(rumble, bot, order).into_iter().enumerate() {
        let opponent = &rumble.standings.bots[pair.opponent].name;
        rows.push_str("<tr>");
        rows.push_str(&cell(&(rank + 1).to_string()));
        rows.push_str(&cell(&logo_tag(logos, opponent)));
        rows.push_str(&cell(&format!(
            "<a href=\"{}\">{}</a>",
            details_href(game, opponent),
            escape(opponent)
        )));
        rows.push_str(&rated(pair.aps, APS_LIMITS));
        rows.push_str(&ci_text(pair.aps_ci));
        rows.push_str(&rated(pair.min_aps, APS_LIMITS));
        rows.push_str(&rated(pair.npp, NPP_LIMITS));
        rows.push_str(&rated(pair.survival, SURVIVAL_LIMITS));
        rows.push_str(&rated(pair.knn_pbi, KNN_PBI_LIMITS));
        rows.push_str(&cell(&pair.battles.to_string()));
        rows.push_str(&cell(&format_ms(pair.last_ms)));
        rows.push_str(&cell(&format!(
            "<a rel=\"nofollow\" href=\"BotCompare?game={}&amp;bota={}&amp;botb={}\">compare</a>",
            encode(game),
            encode(&score.name),
            encode(opponent)
        )));
        rows.push_str(&cell(&format!(
            "<a rel=\"nofollow\" href=\"{}\">details</a>",
            battles_href(game, &score.name, opponent)
        )));
        rows.push_str("</tr>\n");
    }
    let summary = summary_table(
        &[
            "Robot",
            "Rumble",
            "Rank",
            "APS",
            "APS CI",
            "PWIN",
            "ANPP",
            "Vote",
            "Survival",
            "Pairings",
            "Battles",
            "Latest Battle",
        ],
        &[
            cell(&format!("{} {}", logo_tag(logos, &score.name), escape(&score.name))),
            cell(&format!(
                "<a href=\"Rankings?game={}\">{}</a>",
                encode(game),
                escape(&rumble.config.title)
            )),
            cell(&format!("{} of {}", bot + 1, rumble.standings.bots.len())),
            rated(score.aps, APS_LIMITS),
            ci_text(score.aps_ci),
            rated(score.pwin, APS_LIMITS),
            rated(score.anpp, NPP_LIMITS),
            number(score.vote),
            rated(score.survival, SURVIVAL_LIMITS),
            cell(&score.pairings.to_string()),
            cell(&score.battles.to_string()),
            cell(&format_ms(score.last_ms)),
        ],
    );
    let body = format!("{summary}<table>\n{}{rows}</table>\n", sort_header(&base, &columns, order));
    page(&score.name, &body)
}

fn sorted_battles<'a>(battles: &'a [BattleSide], order: &Order) -> Vec<&'a BattleSide> {
    let mut rows: Vec<&BattleSide> = battles.iter().collect();
    sort_rows(&mut rows, order, |battle, column| {
        Some(match column {
            "Date" => Key::Number(battle.time_ms as f64),
            "APS" => Key::Number(battle_aps(battle.score, battle.opponent_score)),
            "Survival" => Key::Number(battle.firsts as f64),
            "Opponent Survival" => Key::Number(battle.opponent_firsts as f64),
            "Score" => Key::Number(battle.score as f64),
            "Opponent Score" => Key::Number(battle.opponent_score as f64),
            "Damage" => Key::Number(battle.bullet_damage as f64),
            "Opponent Damage" => Key::Number(battle.opponent_bullet_damage as f64),
            "Uploader" => Key::Text(battle.uploader.clone()),
            "Client" => Key::Text(battle.client.clone()),
            _ => return None,
        })
    });
    rows
}

/// A pairing's scores and its battles, from the side of one robot. `shown` is the limit the page was
/// asked for, if any, and `battles` holds what that limit allowed.
pub fn battle_details(
    rumble: &RumbleView,
    bot: usize,
    pairing: &PairingView,
    battles: &[BattleSide],
    shown: Option<usize>,
    logos: &Logos,
    order: &Order,
) -> String {
    let game = &rumble.config.name;
    let name = &rumble.standings.bots[bot].name;
    let opponent = &rumble.standings.bots[pairing.opponent].name;
    let rounds = f64::from(rumble.config.rounds);
    let survival = |firsts: i64| 100.0 * firsts as f64 / rounds;

    let summary = summary_table(
        &[
            "Robot",
            "Opponent",
            "APS",
            "APS CI",
            "Min APS",
            "NPP",
            "Survival",
            "KNNPBI",
            "Battles",
            "Latest Battle",
        ],
        &[
            cell(&robot_link(logos, game, name)),
            cell(&robot_link(logos, game, opponent)),
            rated(pairing.aps, APS_LIMITS),
            ci_text(pairing.aps_ci),
            rated(pairing.min_aps, APS_LIMITS),
            rated(pairing.npp, NPP_LIMITS),
            rated(pairing.survival, SURVIVAL_LIMITS),
            rated(pairing.knn_pbi, KNN_PBI_LIMITS),
            cell(&pairing.battles.to_string()),
            cell(&format_ms(pairing.last_ms)),
        ],
    );

    let columns = [
        "Date",
        "APS",
        "Survival",
        "Opponent Survival",
        "Score",
        "Opponent Score",
        "Damage",
        "Opponent Damage",
        "Uploader",
        "Client",
    ];
    let mut base = battles_href(game, name, opponent);
    if let Some(shown) = shown {
        base.push_str(&format!("&amp;limit={shown}"));
    }
    let mut rows = String::new();
    for battle in sorted_battles(battles, order) {
        rows.push_str("<tr>");
        rows.push_str(&cell(&format_ms(battle.time_ms)));
        rows.push_str(&rated(battle_aps(battle.score, battle.opponent_score), APS_LIMITS));
        rows.push_str(&rated(survival(battle.firsts), SURVIVAL_LIMITS));
        rows.push_str(&number(survival(battle.opponent_firsts)));
        rows.push_str(&cell(&battle.score.to_string()));
        rows.push_str(&cell(&battle.opponent_score.to_string()));
        rows.push_str(&cell(&battle.bullet_damage.to_string()));
        rows.push_str(&cell(&battle.opponent_bullet_damage.to_string()));
        rows.push_str(&cell(&escape(&battle.uploader)));
        rows.push_str(&cell(&escape(&battle.client)));
        rows.push_str("</tr>\n");
    }

    let mut notes = String::new();
    if let Some(bots) = rumble.config.melee_bots {
        notes.push_str(&format!(
            "<p>Each row is these two robots' scores in one battle of {bots} robots.</p>\n"
        ));
    }
    if battles.len() < pairing.battles as usize {
        notes.push_str(&format!(
            "<p>The newest {} of {} battles. <a rel=\"nofollow\" href=\"{}&amp;limit={}\">Show all</a></p>\n",
            battles.len(),
            pairing.battles,
            battles_href(game, name, opponent),
            pairing.battles,
        ));
    }
    let body =
        format!("{summary}{notes}<table>\n{}{rows}</table>\n", sort_header(&base, &columns, order));
    page(&format!("{name} against {opponent}"), &body)
}

/// One robot's scores and pairings as JSON.
pub fn bot_details_json(rumble: &RumbleView, bot: usize, logos: &Logos, order: &Order) -> Value {
    let score = &rumble.standings.bots[bot];
    let pairings: Vec<Value> = sorted_pairings(rumble, bot, order)
        .into_iter()
        .enumerate()
        .map(|(rank, pair)| {
            let opponent = &rumble.standings.bots[pair.opponent].name;
            json!({
                "name": opponent,
                "logo": logo_path(logos, opponent),
                "rank": rank + 1,
                "APS": pair.aps,
                "APS_CI": pair.aps_ci,
                "minAPS": pair.min_aps,
                "NPP": pair.npp,
                "survival": pair.survival,
                "KNNPBI": pair.knn_pbi,
                "battles": pair.battles,
                "latest": format_ms(pair.last_ms),
            })
        })
        .collect();
    json!({
        "name": score.name,
        "logo": logo_path(logos, &score.name),
        "APS": score.aps,
        "APS_CI": score.aps_ci,
        "PWIN": score.pwin,
        "ANPP": score.anpp,
        "vote": score.vote,
        "survival": score.survival,
        "pairings": score.pairings,
        "battles": score.battles,
        "latest": format_ms(score.last_ms),
        "pairingsList": pairings,
    })
}

/// Two robots side by side against the opponents they have both fought.
pub fn bot_compare(rumble: &RumbleView, bot_a: usize, bot_b: usize, order: &Order) -> String {
    let game = &rumble.config.name;
    let standings = &rumble.standings;
    let (name_a, name_b) = (&standings.bots[bot_a].name, &standings.bots[bot_b].name);

    struct Row<'a> {
        opponent: &'a str,
        a: &'a PairingView,
        b: &'a PairingView,
    }
    let mut rows: Vec<Row> = Vec::new();
    for a in &standings.pairings[bot_a] {
        let b = standings.pairings[bot_b].iter().find(|b| b.opponent == a.opponent);
        if let Some(b) = b {
            rows.push(Row { opponent: &standings.bots[a.opponent].name, a, b });
        }
    }
    sort_rows(&mut rows, order, |row, column| {
        Some(match column {
            "Competitor" => Key::Text(row.opponent.to_string()),
            "APS A" => Key::Number(row.a.aps),
            "APS B" => Key::Number(row.b.aps),
            "Survival A" => Key::Number(row.a.survival),
            "Survival B" => Key::Number(row.b.survival),
            "Diff APS" => Key::Number(row.a.aps - row.b.aps),
            "Diff Survival" => Key::Number(row.a.survival - row.b.survival),
            _ => return None,
        })
    });

    let columns =
        ["Competitor", "APS A", "APS B", "Diff APS", "Survival A", "Survival B", "Diff Survival"];
    let base = format!(
        "BotCompare?game={}&amp;bota={}&amp;botb={}",
        encode(game),
        encode(name_a),
        encode(name_b)
    );
    let mut table = String::new();
    for row in &rows {
        table.push_str("<tr>");
        table.push_str(&cell(&format!(
            "<a href=\"{}\">{}</a>",
            details_href(game, row.opponent),
            escape(row.opponent)
        )));
        table.push_str(&number(row.a.aps));
        table.push_str(&number(row.b.aps));
        table.push_str(&number(row.a.aps - row.b.aps));
        table.push_str(&number(row.a.survival));
        table.push_str(&number(row.b.survival));
        table.push_str(&number(row.a.survival - row.b.survival));
        table.push_str("</tr>\n");
    }
    let body = format!(
        "<p>A is <a href=\"{}\">{}</a> and B is <a href=\"{}\">{}</a>.</p>\n<table>\n{}{table}</table>\n",
        details_href(game, name_a),
        escape(name_a),
        details_href(game, name_b),
        escape(name_b),
        sort_header(&base, &columns, order),
    );
    page(&format!("{name_a} compared with {name_b}"), &body)
}

/// How much has been uploaded, and by whom.
pub fn rumble_stats(snapshot: &Snapshot) -> String {
    let mut body = String::from(
        "<table>\n<tr><th>Rumble</th><th>Participants</th><th>Results</th><th>Latest battle</th></tr>\n",
    );
    for rumble in &snapshot.rumbles {
        body.push_str(&format!(
            "<tr><td><a href=\"Rankings?game={}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            encode(&rumble.config.name),
            escape(&rumble.config.title),
            rumble.standings.bots.len(),
            rumble.total_battles,
            format_ms(rumble.last_battle_ms),
        ));
    }
    body.push_str("</table>\n");
    for rumble in snapshot.rumbles.iter().filter(|rumble| !rumble.uploaders.is_empty()) {
        body.push_str(&format!(
            "<h2>{}</h2>\n<table>\n<tr><th>Uploader</th><th>Results</th><th>Latest battle</th></tr>\n",
            escape(&rumble.config.title)
        ));
        let mut uploaders: Vec<_> = rumble.uploaders.iter().collect();
        uploaders.sort_by_key(|uploader| std::cmp::Reverse(uploader.battles));
        for uploader in uploaders {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                escape(&uploader.name),
                uploader.battles,
                format_ms(uploader.last_ms)
            ));
        }
        body.push_str("</table>\n");
    }
    page("Statistics", &body)
}

/// What each score means.
pub fn score_explanation() -> String {
    page("Score explanation", include_str!("../docs/scoring.html"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_covers_html_characters() {
        assert_eq!(escape("<a href='x'>&\""), "&lt;a href=&#39;x&#39;&gt;&amp;&quot;");
    }

    #[test]
    fn encode_keeps_names_safe_in_urls() {
        assert_eq!(encode("a.One_2 1.0-b"), "a.One_2%201.0-b");
        assert_eq!(encode("x&y=z"), "x%26y%3Dz");
    }

    #[test]
    fn order_reads_the_direction() {
        let order = Order::parse(Some(&"-APS".to_string()), "Vote");
        assert_eq!(order.column, "APS");
        assert!(order.ascending);
        let default = Order::parse(None, "Vote");
        assert_eq!(default.column, "Vote");
        assert!(!default.ascending);
    }

    #[test]
    fn sorting_is_highest_first_unless_asked() {
        let mut rows = [1.0, 3.0, 2.0];
        let key = |value: &f64, _: &str| Some(Key::Number(*value));
        sort_rows(&mut rows, &Order::parse(None, "x"), key);
        assert_eq!(rows, [3.0, 2.0, 1.0]);
        sort_rows(&mut rows, &Order::parse(None, "-x"), key);
        assert_eq!(rows, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn scores_are_coloured_outside_their_limits() {
        assert!(rated(39.9, APS_LIMITS).contains("class=\"bad\""));
        assert_eq!(rated(40.0, APS_LIMITS), "<td>40.00</td>");
        assert_eq!(rated(60.0, APS_LIMITS), "<td>60.00</td>");
        assert!(rated(60.1, APS_LIMITS).contains("class=\"good\""));
        assert!(rated(65.0, NPP_LIMITS).find("class").is_none());
        assert!(rated(-0.2, KNN_PBI_LIMITS).contains("class=\"bad\""));
    }

    #[test]
    fn the_header_links_flip_the_sorted_column() {
        let header = sort_header("R?game=g", &["APS", "Vote"], &Order::parse(None, "APS"));
        assert!(header.contains("order=-APS"));
        assert!(header.contains("order=Vote"));
        assert!(header.contains("class=\"sortedby\""));
    }
}
