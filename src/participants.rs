//! Participant lists: which robots are in a rumble, and where clients can download them.
//!
//! A list has one `package.Robot version,url` per line, which is the format of the RoboRumble
//! participant pages on the RoboWiki. Lines between `<pre>` and `</pre>` count if the text has
//! those tags, and otherwise every line does.
//!
//! The lists are files in this repository for now. To read one from a web page instead, such as a
//! wiki page, add a `Url` variant to [`Source`] and a branch to [`Source::fetch`]. Nothing else
//! has to change: clients always get the list from this server (`/Participants`), and the engine
//! keeps the last good list when a fetch fails.

use std::path::PathBuf;

use serde::Deserialize;

/// Where a rumble's participant list comes from.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A file on this machine, usually in this repository.
    File(PathBuf),
}

impl Source {
    /// Reads the current text of the list.
    pub fn fetch(&self) -> Result<String, String> {
        match self {
            Source::File(path) => std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display())),
        }
    }
}

/// One robot on a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    /// The name and version, like `claude.Umbra 0.5`.
    pub name: String,
    /// Where to download the jar, or a repository id as the client also allows.
    pub link: String,
}

/// The result of parsing a list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedList {
    pub participants: Vec<Participant>,
    /// Lines that are not valid entries, to be logged.
    pub invalid: Vec<String>,
}

/// Parses the text of a list. A robot listed twice keeps its first entry.
pub fn parse_list(text: &str) -> ParsedList {
    let mut list = ParsedList::default();
    for line in entry_lines(text) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_entry(line) {
            Some(entry) => {
                if !list.participants.iter().any(|other| other.name == entry.name) {
                    list.participants.push(entry);
                }
            }
            None => list.invalid.push(line.to_string()),
        }
    }
    list
}

/// The lines between the `<pre>` tags if there are any, and otherwise all of them.
fn entry_lines(text: &str) -> Vec<&str> {
    let has_tags = text.lines().any(|line| line.contains("<pre>"));
    if !has_tags {
        return text.lines().collect();
    }
    let mut inside = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.contains("</pre>") {
            inside = false;
        } else if line.contains("<pre>") {
            inside = true;
        } else if inside {
            lines.push(line);
        }
    }
    lines
}

/// Applies the same rules as the client, which ignores any other line.
fn parse_entry(line: &str) -> Option<Participant> {
    let (name, link) = line.split_once(',')?;
    let (name, link) = (name.trim(), link.trim());
    if link.contains(',') {
        return None;
    }
    let (robot, version) = name.split_once(' ')?;
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    if robot.is_empty() || !robot.chars().all(word) {
        return None;
    }
    if version.is_empty() || !version.chars().all(|c| word(c) || c == '-') {
        return None;
    }
    let is_url = link.starts_with("http");
    let is_id = !link.is_empty() && link.chars().all(|c| c.is_ascii_digit());
    if !is_url && !is_id {
        return None;
    }
    Some(Participant { name: name.to_string(), link: link.to_string() })
}

/// Writes a list in the form the client downloads.
pub fn to_client_text(participants: &[Participant]) -> String {
    let mut text = String::from("<pre>\n");
    for participant in participants {
        text.push_str(&participant.name);
        text.push(',');
        text.push_str(&participant.link);
        text.push('\n');
    }
    text.push_str("</pre>\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &ParsedList) -> Vec<&str> {
        list.participants.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn parses_a_plain_file() {
        let list = parse_list("a.One 1.0,http://x/a.jar\n\nb.Two 2_rc-1,12345\n");
        assert_eq!(names(&list), ["a.One 1.0", "b.Two 2_rc-1"]);
        assert_eq!(list.participants[1].link, "12345");
        assert!(list.invalid.is_empty());
    }

    #[test]
    fn reads_only_inside_pre_tags_like_a_wiki_page() {
        let page = "Some wiki text, with a comma\n<pre>\na.One 1.0,http://x/a.jar\n</pre>\nb.Two 1,http://x/b.jar\n";
        let list = parse_list(page);
        assert_eq!(names(&list), ["a.One 1.0"]);
        assert!(list.invalid.is_empty());
    }

    #[test]
    fn reports_invalid_lines() {
        let list = parse_list("a.One,http://x\na.One 1.0\na.One 1.0,ftp://x\na.<b> 1,http://x\n");
        assert!(list.participants.is_empty());
        assert_eq!(list.invalid.len(), 4);
    }

    #[test]
    fn a_robot_listed_twice_keeps_its_first_entry() {
        let list = parse_list("a.One 1,http://first\na.One 1,http://second\n");
        assert_eq!(list.participants.len(), 1);
        assert_eq!(list.participants[0].link, "http://first");
    }

    #[test]
    fn client_text_parses_back() {
        let list = parse_list("a.One 1,http://x/a.jar\nb.Two 2,http://x/b.jar\n");
        assert_eq!(parse_list(&to_client_text(&list.participants)), list);
    }
}
