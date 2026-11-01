//! The SQLite database. Its one table of record is `battle`, with one row per uploaded result.
//!
//! Nothing derived is stored. The scores are rebuilt from the battles on every start, so a change
//! to a scoring formula applies to all past battles, and removing a bad uploader's results is a
//! `DELETE` and a restart.
//!
//! The primary key is what identifies a result, so sending the same result twice stores it once. It
//! also keeps a pairing's battles next to each other in time order, so reading them does not depend
//! on how many other battles there are.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

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

/// One stored battle from the side of one robot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleSide {
    pub time_ms: i64,
    pub score: i64,
    pub opponent_score: i64,
    pub firsts: i64,
    pub opponent_firsts: i64,
    pub bullet_damage: i64,
    pub opponent_bullet_damage: i64,
    pub uploader: String,
    pub client: String,
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

    /// Opens a database for reading, which can run alongside the connection that writes to it.
    pub fn open_read_only(path: &Path) -> rusqlite::Result<Store> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
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

    /// Returns the newest battles of a pairing in a rumble, newest first, from the side of `bot`.
    pub fn battles_between(
        &self,
        rumble: &str,
        bot: &str,
        opponent: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<BattleSide>> {
        let id_of = |table: &str, name: &str| -> rusqlite::Result<Option<u32>> {
            self.connection
                .query_row(
                    &format!("SELECT id FROM {table} WHERE name = ?1"),
                    params![name],
                    |row| row.get(0),
                )
                .optional()
        };
        let ids = (id_of("rumble", rumble)?, id_of("bot", bot)?, id_of("bot", opponent)?);
        let (Some(rumble), Some(bot), Some(opponent)) = ids else { return Ok(Vec::new()) };

        let mut statement = self.connection.prepare(
            "SELECT time_ms, lo_score, hi_score, lo_firsts, hi_firsts,
                    lo_bullet_damage, hi_bullet_damage, uploader.name, client
             FROM battle JOIN uploader ON uploader.id = battle.uploader
             WHERE rumble = ?1 AND lo = ?2 AND hi = ?3
             ORDER BY time_ms DESC LIMIT ?4",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let lo_first = bot < opponent;
        let (lo, hi) = if lo_first { (bot, opponent) } else { (opponent, bot) };
        let rows = statement.query_map(params![rumble, lo, hi, limit], |row| {
            // Each pair of columns has the lower id's number first.
            let pair = |first: usize| -> rusqlite::Result<(i64, i64)> {
                let (a, b) = (row.get(first)?, row.get(first + 1)?);
                Ok(if lo_first { (a, b) } else { (b, a) })
            };
            let (score, opponent_score) = pair(1)?;
            let (firsts, opponent_firsts) = pair(3)?;
            let (bullet_damage, opponent_bullet_damage) = pair(5)?;
            Ok(BattleSide {
                time_ms: row.get(0)?,
                score,
                opponent_score,
                firsts,
                opponent_firsts,
                bullet_damage,
                opponent_bullet_damage,
                uploader: row.get(7)?,
                client: row.get(8)?,
            })
        })?;
        rows.collect()
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

    /// A store with a rumble, an uploader and two robots, and rows that use their ids.
    fn store_with_battles() -> (tempfile::TempDir, std::path::PathBuf, Store) {
        let (dir, mut store) = open();
        let path = dir.path().join("test.db");
        let rumble = store.intern(Names::Rumble, "duel").unwrap();
        let lo = store.intern(Names::Bot, "a.One 1").unwrap();
        let hi = store.intern(Names::Bot, "b.Two 1").unwrap();
        let uploader = store.intern(Names::Uploader, "Tester").unwrap();
        let rows: Vec<BattleRow> = (1..=3)
            .map(|time_ms| {
                let mut row = row(time_ms * 100);
                row.rumble = rumble;
                row.uploader = uploader;
                row.scores.lo = lo;
                row.scores.hi = hi;
                row.scores.time_ms = time_ms;
                row
            })
            .collect();
        store.insert_battles(&rows).unwrap();
        (dir, path, store)
    }

    #[test]
    fn a_pairing_reads_back_newest_first_from_either_side() {
        let (_dir, _path, store) = store_with_battles();
        let from_lo = store.battles_between("duel", "a.One 1", "b.Two 1", 10).unwrap();
        let times: Vec<i64> = from_lo.iter().map(|battle| battle.time_ms).collect();
        assert_eq!(times, [3, 2, 1]);
        assert_eq!(
            from_lo[0],
            BattleSide {
                time_ms: 3,
                score: 300,
                opponent_score: 10,
                firsts: 3,
                opponent_firsts: 4,
                bullet_damage: 6,
                opponent_bullet_damage: 7,
                uploader: "Tester".to_string(),
                client: "1.11.2".to_string(),
            }
        );

        let from_hi = store.battles_between("duel", "b.Two 1", "a.One 1", 10).unwrap();
        assert_eq!(from_hi[0].score, 10);
        assert_eq!(from_hi[0].opponent_score, 300);
        assert_eq!(from_hi[0].firsts, 4);
        assert_eq!(from_hi[0].bullet_damage, 7);
    }

    #[test]
    fn the_limit_keeps_the_newest_battles() {
        let (_dir, _path, store) = store_with_battles();
        let battles = store.battles_between("duel", "a.One 1", "b.Two 1", 2).unwrap();
        assert_eq!(battles.iter().map(|battle| battle.time_ms).collect::<Vec<_>>(), [3, 2]);
    }

    #[test]
    fn unknown_names_have_no_battles() {
        let (_dir, _path, store) = store_with_battles();
        assert!(store.battles_between("typo", "a.One 1", "b.Two 1", 10).unwrap().is_empty());
        assert!(store.battles_between("duel", "a.One 1", "nobody", 10).unwrap().is_empty());
    }

    #[test]
    fn a_read_only_connection_sees_what_the_writer_committed() {
        let (_dir, path, mut store) = store_with_battles();
        let reader = Store::open_read_only(&path).unwrap();
        assert_eq!(reader.battles_between("duel", "a.One 1", "b.Two 1", 10).unwrap().len(), 3);

        let mut newer = row(1);
        newer.scores.time_ms = 4;
        (newer.rumble, newer.uploader) = (1, 1);
        (newer.scores.lo, newer.scores.hi) = (1, 2);
        store.insert_battles(&[newer]).unwrap();
        assert_eq!(reader.battles_between("duel", "a.One 1", "b.Two 1", 10).unwrap().len(), 4);
    }
}
