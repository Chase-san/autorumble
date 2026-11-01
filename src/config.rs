//! The server's configuration, in the `config/` directory of the repository:
//!
//! - `config/server.toml` has the server's own settings.
//! - `config/rumbles/NAME.toml` defines the rumble called NAME, one file per rumble.
//!
//! A result is only stored if it matches its rumble's definition here, so a typo in a client's
//! configuration cannot create a new rumble, and a client with the wrong settings cannot add results
//! that do not belong. Relative paths are relative to the directory the server is started in.
//!
//! A single file also works, with `[[rumble]]` tables holding the definitions and each one giving its
//! `name`. The tests and tools use this for their throwaway servers.

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
    /// A directory of robot jars to serve, for robots that are not hosted anywhere else.
    #[serde(default)]
    pub robots: Option<PathBuf>,
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
    #[serde(rename = "rumble", default)]
    pub rumbles: Vec<RumbleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RumbleConfig {
    /// The game name, which is also the name of the client's configuration file. In a rumble's own
    /// file, this comes from the file's name.
    #[serde(default)]
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

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// Reads one rumble's file, which gets its name from the file's name.
fn read_rumble(path: &Path) -> Result<RumbleConfig, String> {
    let mut rumble: RumbleConfig =
        toml::from_str(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    if !rumble.name.is_empty() && rumble.name != name {
        return Err(format!("{} says its name is {}", path.display(), rumble.name));
    }
    rumble.name = name.to_string();
    Ok(rumble)
}

impl Config {
    /// Reads a configuration directory, or a single file.
    pub fn load(path: &Path) -> Result<Config, String> {
        if !path.is_dir() {
            return Config::parse(&read(path)?).map_err(|e| format!("{}: {e}", path.display()));
        }

        let server = path.join("server.toml");
        let mut config: Config =
            toml::from_str(&read(&server)?).map_err(|e| format!("{}: {e}", server.display()))?;

        let directory = path.join("rumbles");
        let entries = std::fs::read_dir(&directory)
            .map_err(|e| format!("cannot read {}: {e}", directory.display()))?;
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|file| file.extension().is_some_and(|extension| extension == "toml"))
            .collect();
        files.sort();
        for file in files {
            config.rumbles.push(read_rumble(&file)?);
        }

        config.validate().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(config)
    }

    /// Reads the text of a single configuration file.
    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.rumbles.is_empty() {
            return Err("no rumbles are defined".to_string());
        }
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

    const SERVER: &str = r#"
        listen = "127.0.0.1:1"
        database = "a.db"
        logos = "logos"
        allowed_clients = ["1.11.2"]
    "#;

    const DEFINITION: &str = r#"
        title = "A rumble"
        field = "800x600"
        rounds = 35
        participants = { file = "a.txt" }
    "#;

    fn single_file(name: &str) -> String {
        format!("{SERVER}\n[[rumble]]\nname = \"{name}\"\n{DEFINITION}")
    }

    /// A configuration directory holding the given rumble files.
    fn directory(rumbles: &[(&str, &str)]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("server.toml"), SERVER).unwrap();
        std::fs::create_dir(directory.path().join("rumbles")).unwrap();
        for (file, text) in rumbles {
            std::fs::write(directory.path().join("rumbles").join(file), text).unwrap();
        }
        directory
    }

    #[test]
    fn the_repository_configuration_is_valid() {
        let config = Config::load(Path::new("config")).unwrap();
        let names: Vec<&str> = config.rumbles.iter().map(|rumble| rumble.name.as_str()).collect();
        assert_eq!(names, ["automelee6", "autorumble", "openmelee6", "openrumble"]);
        assert!(config.rumbles.iter().all(|rumble| rumble.hide_names));
        let melee = &config.rumbles[0];
        assert_eq!(melee.melee_bots, Some(6));
        assert_eq!(melee.field, "800x800");
    }

    #[test]
    fn a_rumble_is_named_after_its_file() {
        let directory = directory(&[
            ("two.toml", DEFINITION),
            ("one.toml", DEFINITION),
            ("notes.txt", "ignored"),
        ]);
        let config = Config::load(directory.path()).unwrap();
        let names: Vec<&str> = config.rumbles.iter().map(|rumble| rumble.name.as_str()).collect();
        assert_eq!(names, ["one", "two"]);
    }

    #[test]
    fn a_file_cannot_claim_another_name() {
        let text = format!("name = \"other\"\n{DEFINITION}");
        let directory = directory(&[("one.toml", &text)]);
        assert!(Config::load(directory.path()).unwrap_err().contains("says its name is other"));
    }

    #[test]
    fn a_bad_file_name_is_refused() {
        let directory = directory(&[("Has Space.toml", DEFINITION)]);
        assert!(Config::load(directory.path()).is_err());
    }

    #[test]
    fn a_directory_needs_at_least_one_rumble() {
        let directory = directory(&[]);
        assert!(Config::load(directory.path()).unwrap_err().contains("no rumbles"));
    }

    #[test]
    fn a_mistake_in_a_rumble_file_names_the_file() {
        let directory = directory(&[("one.toml", "rounds = \"many\"")]);
        assert!(Config::load(directory.path()).unwrap_err().contains("one.toml"));
    }

    #[test]
    fn defaults_apply() {
        let config = Config::parse(&single_file("one")).unwrap();
        assert_eq!(config.standings_seconds, 10);
        assert!(!config.rumbles[0].is_melee());
    }

    #[test]
    fn rejects_bad_names_and_fields() {
        assert!(Config::parse(&single_file("Has Space")).is_err());
        let bad_field = single_file("one").replace("800x600", "800");
        assert!(Config::parse(&bad_field).is_err());
    }

    #[test]
    fn rejects_a_rumble_declared_twice() {
        let rumble = single_file("one");
        let twice = rumble.clone() + &rumble[rumble.find("[[rumble]]").unwrap()..];
        assert!(Config::parse(&twice).is_err());
    }
}
