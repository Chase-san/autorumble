//! The server's configuration file. `autorumble.toml` in the repository is the documented example.
//!
//! Rumbles are declared here, so a typo in a client's configuration cannot create a new rumble.
//! Relative paths are relative to the directory the server is started in.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::participants::Source;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The address to listen on, such as `127.0.0.1:8652`.
    pub listen: String,
    /// The SQLite file holding every battle.
    pub database: PathBuf,
    /// The directory of author logos.
    pub logos: PathBuf,
    /// How often to recompute the standings of a rumble with new battles.
    #[serde(default = "default_standings_seconds")]
    pub standings_seconds: u64,
    /// How often to read the participant lists and logos again.
    #[serde(default = "default_participants_seconds")]
    pub participants_seconds: u64,
    /// Battles older than this are rejected.
    #[serde(default = "default_max_battle_age_hours")]
    pub max_battle_age_hours: u64,
    /// The Robocode versions allowed to upload.
    pub allowed_clients: Vec<String>,
    #[serde(rename = "rumble")]
    pub rumbles: Vec<RumbleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RumbleConfig {
    /// The game name. It is also the name of the client's configuration file.
    pub name: String,
    /// The name shown on the pages.
    pub title: String,
    /// The battlefield as `WIDTHxHEIGHT`.
    pub field: String,
    /// The number of rounds per battle.
    pub rounds: u32,
    /// The number of robots per battle in a melee rumble. Leave it out for 1v1.
    #[serde(default)]
    pub melee_bots: Option<u32>,
    /// Whether robots must not see the names of their enemies.
    #[serde(default)]
    pub hide_names: bool,
    /// Where the participant list comes from.
    pub participants: Source,
}

impl RumbleConfig {
    pub fn is_melee(&self) -> bool {
        self.melee_bots.is_some()
    }
}

fn default_standings_seconds() -> u64 {
    10
}

fn default_participants_seconds() -> u64 {
    300
}

fn default_max_battle_age_hours() -> u64 {
    24
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Config::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        let mut names = HashSet::new();
        for rumble in &self.rumbles {
            let name = &rumble.name;
            if name.is_empty()
                || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            {
                return Err(format!("rumble name {name:?} must be lowercase letters and digits"));
            }
            if !names.insert(name) {
                return Err(format!("rumble {name} is declared twice"));
            }
            if rumble.rounds == 0 {
                return Err(format!("rumble {name} needs at least one round"));
            }
            let size = rumble.field.split_once('x');
            if !size.is_some_and(|(w, h)| w.parse::<u32>().is_ok() && h.parse::<u32>().is_ok()) {
                return Err(format!("rumble {name} has a bad field {:?}", rumble.field));
            }
            if rumble.melee_bots.is_some_and(|bots| bots < 3) {
                return Err(format!("melee rumble {name} needs at least 3 robots per battle"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUMBLE: &str = r#"
        listen = "127.0.0.1:1"
        database = "a.db"
        logos = "logos"
        allowed_clients = ["1.11.2"]
        [[rumble]]
        name = "NAME"
        title = "A rumble"
        field = "800x600"
        rounds = 35
        participants = { file = "a.txt" }
    "#;

    #[test]
    fn the_example_file_is_valid() {
        let config = Config::parse(include_str!("../autorumble.toml")).unwrap();
        assert_eq!(config.rumbles.len(), 4);
        assert!(config.rumbles.iter().all(|rumble| rumble.hide_names));
        let melee = config.rumbles.iter().find(|r| r.name == "automelee6").unwrap();
        assert_eq!(melee.melee_bots, Some(6));
        assert_eq!(melee.field, "800x800");
    }

    #[test]
    fn defaults_apply() {
        let config = Config::parse(&RUMBLE.replace("NAME", "one")).unwrap();
        assert_eq!(config.standings_seconds, 10);
        assert!(!config.rumbles[0].is_melee());
    }

    #[test]
    fn rejects_bad_names_and_fields() {
        assert!(Config::parse(&RUMBLE.replace("NAME", "Has Space")).is_err());
        let bad_field = RUMBLE.replace("NAME", "one").replace("800x600", "800");
        assert!(Config::parse(&bad_field).is_err());
    }

    #[test]
    fn rejects_a_rumble_declared_twice() {
        let rumble = RUMBLE.replace("NAME", "one");
        let twice = rumble.clone() + &rumble[rumble.find("[[rumble]]").unwrap()..];
        assert!(Config::parse(&twice).is_err());
    }
}
