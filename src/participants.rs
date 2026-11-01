//! Participant lists: which robots are in a rumble, and where clients can download them.
//!
//! A list has one `package.Robot version,url` per line, which is the format of the RoboRumble
//! participant pages on the RoboWiki. It comes from a file or a web server. Open and close tags mark
//! where it is in the text, so one page can hold the lists of several rumbles under different tags.
//!
//! Clients always get the list from this server (`/Participants`). When a list cannot be read, the
//! engine keeps the last good one.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use serde::Deserialize;

const DEFAULT_OPEN: &str = "<pre>";
const DEFAULT_CLOSE: &str = "</pre>";
/// How long one fetch can take.
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// The shortest time between fetches of a page, however often the lists are read.
const MIN_FETCH_SECONDS: u64 = 60;
const USER_AGENT: &str = "autorumble (https://github.com/Chase-san/autorumble)";

/// Where a rumble's participant list comes from, and what marks it in the text.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// A file on this machine, usually in this repository.
    pub file: Option<PathBuf>,
    /// A page on a web server.
    pub url: Option<String>,
    /// The text just before the list, which is `<pre>` unless this is set.
    pub open: Option<String>,
    /// The text just after the list, which is `</pre>` unless this is set.
    pub close: Option<String>,
}

impl Source {
    /// Checks that the settings make sense.
    pub fn check(&self) -> Result<(), String> {
        match (&self.file, &self.url) {
            (Some(_), None) => {}
            (None, Some(url)) if url.starts_with("http://") || url.starts_with("https://") => {}
            (None, Some(url)) => return Err(format!("{url} is not an http or https address")),
            _ => return Err("participants needs either a file or a url".to_string()),
        }
        match (self.open.as_deref(), self.close.as_deref()) {
            (None, None) => Ok(()),
            (Some(open), Some(close)) if !open.is_empty() && !close.is_empty() => Ok(()),
            _ => Err("open and close must both be set, and cannot be empty".to_string()),
        }
    }

    /// Reads the text of a list that comes from a file. Lists from a web server arrive through
    /// [`fetch_pages`] instead.
    pub fn read_file(&self) -> Option<Result<String, String>> {
        let path = self.file.as_ref()?;
        Some(
            std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display())),
        )
    }

    /// Finds the list in the text and parses it. Only a file with no tags set can be all list, so
    /// a page that has changed or a site's error page never counts as a list.
    pub fn parse(&self, text: &str) -> Result<ParsedList, String> {
        let plain_file = self.file.is_some() && self.open.is_none();
        if plain_file && !text.contains(DEFAULT_OPEN) {
            return Ok(parse_lines(text.lines()));
        }
        let open = self.open.as_deref().unwrap_or(DEFAULT_OPEN);
        let close = self.close.as_deref().unwrap_or(DEFAULT_CLOSE);
        let blocks = between_tags(text, open, close)?;
        Ok(parse_lines(blocks.iter().flat_map(|block| block.lines())))
    }
}

/// Every part of the text between the tags. A cut-off text fails, so it cannot shorten a list.
fn between_tags<'a>(text: &'a str, open: &str, close: &str) -> Result<Vec<&'a str>, String> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        let inside = &rest[start + open.len()..];
        let end = inside.find(close).ok_or_else(|| format!("{open} has no {close} after it"))?;
        blocks.push(&inside[..end]);
        rest = &inside[end + close.len()..];
    }
    if blocks.is_empty() {
        return Err(format!("the text has no {open}"));
    }
    Ok(blocks)
}

/// Fetches one page.
fn fetch(url: &str) -> Result<String, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        .user_agent(USER_AGENT)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent.get(url).call().map_err(|e| format!("cannot fetch {url}: {e}"))?;
    response.body_mut().read_to_string().map_err(|e| format!("cannot read {url}: {e}"))
}

/// A page's address, and its text or why there is none.
pub type FetchedPage = (String, Result<String, String>);

/// Fetches the pages whenever clients are active, and sends each one to the engine. This runs on its
/// own thread, so a slow site cannot hold up uploads, and it ends when the engine goes away.
pub fn fetch_pages(
    urls: &[String],
    seconds: u64,
    activity: &Receiver<()>,
    pages: &Sender<FetchedPage>,
) {
    let every = Duration::from_secs(seconds.max(MIN_FETCH_SECONDS));
    fetch_rounds(urls, every, activity, pages, fetch);
}

/// Each signal of client activity brings one round of fetches, so an idle server asks nothing of the
/// sites. The requests are spread evenly over the round, because some sites refuse requests that
/// come close together. A round takes the given time, which limits how often a page is fetched
/// however busy the clients are.
fn fetch_rounds(
    urls: &[String],
    every: Duration,
    activity: &Receiver<()>,
    pages: &Sender<FetchedPage>,
    fetch: impl Fn(&str) -> Result<String, String>,
) {
    let pause = every / urls.len().max(1) as u32;
    while activity.recv().is_ok() {
        for url in urls {
            if pages.send((url.clone(), fetch(url))).is_err() {
                return;
            }
            std::thread::sleep(pause);
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

/// Parses the lines of a list. A robot listed twice keeps its first entry.
pub fn parse_lines<'a>(lines: impl Iterator<Item = &'a str>) -> ParsedList {
    let mut list = ParsedList::default();
    for line in lines {
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

/// Writes a list as plain lines.
pub fn to_plain_text(participants: &[Participant]) -> String {
    let mut text = String::new();
    for participant in participants {
        text.push_str(&participant.name);
        text.push(',');
        text.push_str(&participant.link);
        text.push('\n');
    }
    text
}

/// Writes a list in the form the client downloads.
pub fn to_client_text(participants: &[Participant]) -> String {
    format!("{DEFAULT_OPEN}\n{}{DEFAULT_CLOSE}\n", to_plain_text(participants))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{channel, sync_channel};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn pages_are_only_fetched_after_client_activity() {
        let (signal, activity) = sync_channel(1);
        let (sender, pages) = channel();
        let urls = ["a".to_string(), "b".to_string()];
        let fetching = std::thread::spawn(move || {
            let every = Duration::from_millis(100);
            fetch_rounds(&urls, every, &activity, &sender, |url| Ok(url.to_string()));
        });

        assert!(pages.recv_timeout(Duration::from_millis(300)).is_err(), "no activity yet");

        signal.send(()).unwrap();
        assert_eq!(pages.recv_timeout(WAIT).unwrap().0, "a");
        assert_eq!(pages.recv_timeout(WAIT).unwrap().0, "b");
        assert!(pages.recv_timeout(Duration::from_millis(300)).is_err(), "idle again");

        drop(signal);
        fetching.join().unwrap();
    }

    #[test]
    fn busy_clients_cannot_make_fetches_more_frequent() {
        let every = Duration::from_millis(100);
        let (signal, activity) = sync_channel(1);
        let (sender, pages) = channel();
        let times = Arc::new(Mutex::new(Vec::new()));
        let fetch_times = times.clone();
        let fetching = std::thread::spawn(move || {
            let urls = ["a".to_string()];
            fetch_rounds(&urls, every, &activity, &sender, |_| {
                fetch_times.lock().unwrap().push(Instant::now());
                Ok(String::new())
            });
        });

        for _ in 0..50 {
            let _ = signal.try_send(());
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(signal);
        fetching.join().unwrap();

        assert!(pages.try_iter().count() >= 2);
        let times = times.lock().unwrap();
        assert!(times.windows(2).all(|pair| pair[1] - pair[0] >= every), "{times:?}");
    }

    fn from_file() -> Source {
        Source { file: Some("list.txt".into()), ..Source::default() }
    }

    fn from_web() -> Source {
        Source { url: Some("http://example.com/list".to_string()), ..Source::default() }
    }

    fn with_tags(source: Source, open: &str, close: &str) -> Source {
        Source { open: Some(open.to_string()), close: Some(close.to_string()), ..source }
    }

    fn names(list: &ParsedList) -> Vec<&str> {
        list.participants.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn parses_a_plain_file() {
        let list = from_file().parse("a.One 1.0,http://x/a.jar\n\nb.Two 2_rc-1,12345\n").unwrap();
        assert_eq!(names(&list), ["a.One 1.0", "b.Two 2_rc-1"]);
        assert_eq!(list.participants[1].link, "12345");
        assert!(list.invalid.is_empty());
    }

    #[test]
    fn reads_only_inside_pre_tags_like_a_wiki_page() {
        let page = "Some wiki text, with a comma\n<pre>\na.One 1.0,http://x/a.jar\n</pre>\nb.Two 1,http://x/b.jar\n";
        for source in [from_file(), from_web()] {
            let list = source.parse(page).unwrap();
            assert_eq!(names(&list), ["a.One 1.0"]);
            assert!(list.invalid.is_empty());
        }
    }

    #[test]
    fn each_rumble_finds_its_list_by_its_own_tags() {
        let page = "\
Intro, with a comma
<!-- open -->
a.One 1,http://x/a.jar
<!-- /open -->
<pre>
c.Pre 1,http://x/c.jar
</pre>
<!-- auto -->
b.Two 1,http://x/b.jar
<!-- /auto -->
";
        let open = with_tags(from_web(), "<!-- open -->", "<!-- /open -->").parse(page).unwrap();
        let auto = with_tags(from_web(), "<!-- auto -->", "<!-- /auto -->").parse(page).unwrap();
        assert_eq!(names(&open), ["a.One 1"]);
        assert_eq!(names(&auto), ["b.Two 1"]);
    }

    #[test]
    fn tags_can_be_any_text_on_any_line() {
        let same = with_tags(from_web(), "----", "----");
        let list = same.parse("x\n----\na.One 1,http://x\n----\ny\n----\nb.Two 1,http://x\n----\n");
        assert_eq!(names(&list.unwrap()), ["a.One 1", "b.Two 1"]);

        let inline = with_tags(from_file(), "[list]", "[/list]");
        assert_eq!(names(&inline.parse("x [list]a.One 1,http://x[/list] y").unwrap()), ["a.One 1"]);
    }

    #[test]
    fn a_page_without_its_tags_is_not_a_list() {
        let robots = "a.One 1,http://x/a.jar\n";
        assert!(from_web().parse(robots).unwrap_err().contains("no <pre>"));
        assert!(with_tags(from_file(), "<a>", "</a>").parse(robots).is_err());
        assert!(from_web().parse("<pre>\na.One 1,http://x\n").unwrap_err().contains("</pre>"));
    }

    #[test]
    fn the_settings_are_checked() {
        assert!(from_file().check().is_ok());
        assert!(with_tags(from_web(), "<a>", "</a>").check().is_ok());

        assert!(Source::default().check().is_err());
        assert!(Source { file: Some("a".into()), ..from_web() }.check().is_err());
        assert!(Source { url: Some("ftp://x".to_string()), ..Source::default() }.check().is_err());
        assert!(Source { open: Some("<a>".to_string()), ..from_web() }.check().is_err());
        assert!(with_tags(from_web(), "", "</a>").check().is_err());
    }

    #[test]
    fn reports_invalid_lines() {
        let text = "a.One,http://x\na.One 1.0\na.One 1.0,ftp://x\na.<b> 1,http://x\n";
        let list = from_file().parse(text).unwrap();
        assert!(list.participants.is_empty());
        assert_eq!(list.invalid.len(), 4);
    }

    #[test]
    fn a_robot_listed_twice_keeps_its_first_entry() {
        let list = from_file().parse("a.One 1,http://first\na.One 1,http://second\n").unwrap();
        assert_eq!(list.participants.len(), 1);
        assert_eq!(list.participants[0].link, "http://first");
    }

    #[test]
    fn written_lists_parse_back() {
        let text = "a.One 1,http://x/a.jar\nb.Two 2,http://x/b.jar\n";
        let list = from_file().parse(text).unwrap();
        assert_eq!(to_plain_text(&list.participants), text);
        assert_eq!(from_web().parse(&to_client_text(&list.participants)).unwrap(), list);
    }
}
