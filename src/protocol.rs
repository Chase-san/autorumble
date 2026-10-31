//! The text format of uploads and their replies, described in `docs/protocol.md`.
//!
//! A result is one line of `key=value` pairs joined by `&`. It looks like a form post, but the
//! client does not encode the values, so robot names contain plain spaces.

use std::collections::HashMap;

/// One robot's numbers in a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub name: String,
    pub score: i64,
    pub bullet_damage: i64,
    /// The number of rounds this robot finished first.
    pub firsts: i64,
}

/// One uploaded result: a pair of robots from one battle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub game: String,
    pub version: String,
    pub client: String,
    pub teams: bool,
    pub melee: bool,
    pub hide_names: bool,
    pub rounds: u32,
    pub field: String,
    pub user: String,
    pub time_ms: i64,
    pub first: Side,
    pub second: Side,
}

/// Parses one result. The error says which field is wrong, and is sent back to the client.
pub fn parse_upload(line: &str) -> Result<Upload, String> {
    let fields: HashMap<&str, &str> =
        line.trim().split('&').filter_map(|field| field.split_once('=')).collect();
    let text = |key: &str| {
        fields.get(key).map(|value| value.to_string()).ok_or_else(|| format!("no {key} field"))
    };
    let number = |key: &str| -> Result<i64, String> {
        text(key)?.parse().map_err(|_| format!("bad {key} field"))
    };
    let flag = |key: &str| fields.get(key).is_some_and(|value| *value == "YES");
    let side =
        |name: &str, score: &str, bullet_damage: &str, firsts: &str| -> Result<Side, String> {
            Ok(Side {
                name: text(name)?,
                score: number(score)?,
                bullet_damage: number(bullet_damage)?,
                firsts: number(firsts)?,
            })
        };

    Ok(Upload {
        game: text("game")?,
        version: text("version")?,
        client: text("client")?,
        teams: flag("teams"),
        melee: flag("melee"),
        hide_names: flag("hidenames"),
        rounds: u32::try_from(number("rounds")?).map_err(|_| "bad rounds field".to_string())?,
        field: text("field")?,
        user: text("user")?,
        time_ms: number("time")?,
        first: side("fname", "fscore", "fbulletd", "fsurvival")?,
        second: side("sname", "sscore", "sbulletd", "ssurvival")?,
    })
}

/// What happened to one result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Newly stored.
    Stored,
    /// Stored by an earlier upload, which makes sending a result twice harmless.
    Duplicate,
    /// Will never be accepted, so the client must not send it again.
    Rejected(String),
    /// Not stored for now, so the client should send it again later.
    Retry(String),
}

/// A priority battle in the form the client reads, like `[a.One_1.0,b.Two_2.0]`.
pub fn priority_line(first: &str, second: &str) -> String {
    format!("[{},{}]", first.replace(' ', "_"), second.replace(' ', "_"))
}

/// The reply to a single upload, for clients without batch support.
///
/// These clients treat any line containing `OK` as delivered and send anything else again, so a
/// rejection has to say `OK` and a temporary failure must not.
pub fn single_reply(outcome: &Outcome, priority: &[String]) -> String {
    let mut reply = String::new();
    for line in priority {
        reply.push_str(line);
        reply.push('\n');
    }
    reply.push_str(&match outcome {
        Outcome::Stored => "OK. Result stored.\n".to_string(),
        Outcome::Duplicate => "OK. Duplicate result ignored.\n".to_string(),
        Outcome::Rejected(reason) => format!("OK. ERROR: {reason}\n"),
        Outcome::Retry(reason) => format!("RETRY: {reason}\n"),
    });
    reply
}

/// The reply to a batch: one status line per result in the same order, then priority battles.
pub fn batch_reply(outcomes: &[Outcome], priority: &[String]) -> String {
    let mut reply = String::new();
    for outcome in outcomes {
        reply.push_str(&match outcome {
            Outcome::Stored => "OK stored\n".to_string(),
            Outcome::Duplicate => "OK duplicate\n".to_string(),
            Outcome::Rejected(reason) => format!("REJECT {reason}\n"),
            Outcome::Retry(reason) => format!("RETRY {reason}\n"),
        });
    }
    for line in priority {
        reply.push_str(line);
        reply.push('\n');
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the Java client sends for one 1v1 result.
    const LINE: &str = "game=autorumble&version=1&client=1.11.2&teams=NOT&melee=NOT&rounds=35\
        &field=800x600&user=Chase&time=1793491199000&fname=a.One 1.0&fscore=3000&fbulletd=900\
        &fsurvival=20&sname=b.Two 2.0&sscore=1000&sbulletd=300&ssurvival=15&hidenames=YES\n";

    #[test]
    fn parses_what_the_client_sends() {
        let upload = parse_upload(LINE).unwrap();
        assert_eq!(upload.game, "autorumble");
        assert_eq!(upload.client, "1.11.2");
        assert!(!upload.melee);
        assert!(upload.hide_names);
        assert_eq!(upload.rounds, 35);
        assert_eq!(upload.field, "800x600");
        assert_eq!(upload.time_ms, 1_793_491_199_000);
        assert_eq!(upload.first.name, "a.One 1.0");
        assert_eq!(upload.first.firsts, 20);
        assert_eq!(upload.second.score, 1000);
    }

    #[test]
    fn names_the_bad_field() {
        assert_eq!(parse_upload("game=x").unwrap_err(), "no version field");
        let bad = LINE.replace("fscore=3000", "fscore=lots");
        assert_eq!(parse_upload(&bad).unwrap_err(), "bad fscore field");
    }

    #[test]
    fn hidden_names_are_off_unless_the_client_says_so() {
        let upload = parse_upload(&LINE.replace("&hidenames=YES", "")).unwrap();
        assert!(!upload.hide_names);
    }

    #[test]
    fn priority_lines_use_underscores() {
        assert_eq!(priority_line("a.One 1.0", "b.Two 2"), "[a.One_1.0,b.Two_2]");
    }

    #[test]
    fn single_replies_follow_the_clients_ok_rule() {
        for outcome in [Outcome::Stored, Outcome::Duplicate, Outcome::Rejected("no".to_string())] {
            assert!(single_reply(&outcome, &[]).contains("OK"));
        }
        assert!(!single_reply(&Outcome::Retry("busy".to_string()), &[]).contains("OK"));
    }

    #[test]
    fn batch_replies_keep_the_order_and_end_with_priority_battles() {
        let reply = batch_reply(
            &[
                Outcome::Stored,
                Outcome::Rejected("no".to_string()),
                Outcome::Retry("busy".to_string()),
            ],
            &["[a_1,b_2]".to_string()],
        );
        assert_eq!(reply, "OK stored\nREJECT no\nRETRY busy\n[a_1,b_2]\n");
    }
}
