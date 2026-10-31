//! The SQLite database. Its one table of record is `battle`, with one row per uploaded result.
//!
//! Nothing derived is stored. The scores are rebuilt from the battles on every start, so a change
//! to a scoring formula applies to all past battles, and removing a bad uploader's results is a
//! `DELETE` and a restart.
//!
//! The primary key is what identifies a result, so sending the same result twice stores it once.

use std::path::Path;

use rusqlite::{Connection, params};

use crate::scoring::{BattleScores, BotId};

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS rumble (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
    CREATE TABLE IF NOT EXISTS bot (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
    CREATE TABLE IF NOT EXISTS uploader (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
    CREATE TABLE IF NOT EXISTS battle (
        rumble INTEGER NOT NULL,
        lo INTEGER NOT NULL,          -- the robot with the lower id
        hi INTEGER NOT NULL,          -- the robot with the higher id
        time_ms INTEGER NOT NULL,     -- when the battle was fought, by the client's clock
        lo_score INTEGER NOT NULL,
        hi_score INTEGER NOT NULL,
        lo_bullet_damage INTEGER NOT NULL,
        hi_bullet_damage INTEGER NOT NULL,
        lo_firsts INTEGER NOT NULL,   -- rounds finished first
        hi_firsts INTEGER NOT NULL,
        uploader INTEGER NOT NULL,
        client TEXT NOT NULL,         -- the Robocode version
        received_ms INTEGER NOT NULL, -- when it arrived, by this server's clock
        PRIMARY KEY (rumble, lo, hi, time_ms, lo_score, hi_score)
    ) WITHOUT ROWID;
";

/// A table that maps names to small ids.
#[derive(Debug, Clone, Copy)]
pub enum Names {
    Rumble,
    Bot,
    Uploader,
}

impl Names {
    fn table(self) -> &'static str {
        match self {
            Names::Rumble => "rumble",
            Names::Bot => "bot",
            Names::Uploader => "uploader",
        }
    }
}

/// One row of the `battle` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleRow {
    pub rumble: u32,
    pub scores: BattleScores,
    pub lo_bullet_damage: i64,
    pub hi_bullet_damage: i64,
    pub uploader: u32,
    pub client: String,
    pub received_ms: i64,
}

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Store> {
        let connection = Connection::open(path)?;
        // WAL lets readers run during writes. NORMAL keeps the database intact through a crash,
        // and a power cut can only lose the last few results.
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(SCHEMA)?;
        Ok(Store { connection })
    }

    /// Returns the id of a name, adding it if it is new.
    pub fn intern(&self, names: Names, name: &str) -> rusqlite::Result<u32> {
        let table = names.table();
        self.connection
            .execute(&format!("INSERT OR IGNORE INTO {table} (name) VALUES (?1)"), params![name])?;
        self.connection.query_row(
            &format!("SELECT id FROM {table} WHERE name = ?1"),
            params![name],
            |row| row.get(0),
        )
    }

    /// Returns every `(id, name)` in a table.
    pub fn all_names(&self, names: Names) -> rusqlite::Result<Vec<(u32, String)>> {
        let table = names.table();
        let mut statement = self.connection.prepare(&format!("SELECT id, name FROM {table}"))?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Stores results in one transaction. Returns, for each one, whether it was new.
    /// If this fails then nothing was stored.
    pub fn insert_battles(&mut self, rows: &[BattleRow]) -> rusqlite::Result<Vec<bool>> {
        let transaction = self.connection.transaction()?;
        let mut new = Vec::with_capacity(rows.len());
        {
            let mut statement = transaction.prepare_cached(
                "INSERT OR IGNORE INTO battle (rumble, lo, hi, time_ms, lo_score, hi_score,
                     lo_bullet_damage, hi_bullet_damage, lo_firsts, hi_firsts,
                     uploader, client, received_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?;
            for row in rows {
                let scores = &row.scores;
                let changed = statement.execute(params![
                    row.rumble,
                    scores.lo,
                    scores.hi,
                    scores.time_ms,
                    scores.lo_score,
                    scores.hi_score,
                    row.lo_bullet_damage,
                    row.hi_bullet_damage,
                    scores.lo_firsts,
                    scores.hi_firsts,
                    row.uploader,
                    row.client,
                    row.received_ms,
                ])?;
                new.push(changed == 1);
            }
        }
        transaction.commit()?;
        Ok(new)
    }

    /// Calls `visit` with every stored battle, in no particular order.
    pub fn for_each_battle(
        &self,
        mut visit: impl FnMut(u32, &BattleScores, u32),
    ) -> rusqlite::Result<()> {
        let mut statement = self.connection.prepare(
            "SELECT rumble, lo, hi, lo_score, hi_score, lo_firsts, hi_firsts, time_ms, uploader
             FROM battle",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let lo: BotId = row.get(1)?;
            let hi: BotId = row.get(2)?;
            let scores = BattleScores {
                lo,
                hi,
                lo_score: row.get(3)?,
                hi_score: row.get(4)?,
                lo_firsts: row.get(5)?,
                hi_firsts: row.get(6)?,
                time_ms: row.get(7)?,
            };
            visit(row.get(0)?, &scores, row.get(8)?);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(lo_score: i64) -> BattleRow {
        BattleRow {
            rumble: 1,
            scores: BattleScores {
                lo: 1,
                hi: 2,
                lo_score,
                hi_score: 10,
                lo_firsts: 3,
                hi_firsts: 4,
                time_ms: 5,
            },
            lo_bullet_damage: 6,
            hi_bullet_damage: 7,
            uploader: 8,
            client: "1.11.2".to_string(),
            received_ms: 9,
        }
    }

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("test.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn interning_gives_the_same_id_for_the_same_name() {
        let (_dir, store) = open();
        let first = store.intern(Names::Bot, "a.One 1").unwrap();
        let second = store.intern(Names::Bot, "b.Two 1").unwrap();
        assert_ne!(first, second);
        assert_eq!(store.intern(Names::Bot, "a.One 1").unwrap(), first);
        assert_eq!(store.all_names(Names::Bot).unwrap().len(), 2);
        assert!(store.all_names(Names::Uploader).unwrap().is_empty());
    }

    #[test]
    fn the_same_result_is_stored_once() {
        let (_dir, mut store) = open();
        assert_eq!(store.insert_battles(&[row(1), row(2)]).unwrap(), [true, true]);
        assert_eq!(store.insert_battles(&[row(2), row(3)]).unwrap(), [false, true]);
        assert_eq!(store.insert_battles(&[row(4), row(4)]).unwrap(), [true, false]);
    }

    #[test]
    fn battles_read_back_as_stored() {
        let (_dir, mut store) = open();
        store.insert_battles(&[row(1)]).unwrap();
        let mut seen = Vec::new();
        store
            .for_each_battle(|rumble, scores, uploader| seen.push((rumble, *scores, uploader)))
            .unwrap();
        assert_eq!(seen, [(1, row(1).scores, 8)]);
    }
}
