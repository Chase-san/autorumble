//! The engine is one thread that owns the database and all the scores.
//!
//! Web handlers never touch that state. An upload is sent to the thread as a job and the handler
//! waits for the reply, while pages read the latest [`Snapshot`], which is immutable. With only one
//! thread able to change anything, the server needs no locks around its data.
//!
//! The thread takes every job waiting in its queue and stores them in one transaction, so many
//! clients uploading one result at a time cost about the same as one client uploading a batch.
//! A result is only added to the scores once its transaction has committed, and the reply is only
//! sent after that, so a client that sees `OK` knows the result is safe.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use fastrand::Rng;
use tokio::sync::oneshot;

use crate::config::{Config, RumbleConfig};
use crate::logos::{self, Logos};
use crate::participants::{self, Participant};
use crate::priority;
use crate::protocol::{self, Outcome, Upload};
use crate::scoring::{BattleScores, BotId, Entrant, Standings, Tally};
use crate::store::{BattleRow, Names, Store};
use crate::time;

/// How many jobs can wait. When the queue is full, uploads get a retry status.
const QUEUE_LENGTH: usize = 4096;
/// The most jobs stored in one transaction.
const MAX_JOBS_PER_COMMIT: usize = 256;
/// How many priority battles to choose each time the standings are computed.
const PRIORITY_QUEUE_LENGTH: usize = 1000;
/// How far ahead of this server's clock a client's clock can be.
const MAX_FUTURE_MS: i64 = 3_600_000;
/// Uploader names are cut to this many characters.
const MAX_USER_CHARS: usize = 64;
/// How long the thread sleeps at most before it checks its timers.
const TICK: Duration = Duration::from_secs(1);

/// The answer to a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// One outcome per line of the job, in the same order.
    pub outcomes: Vec<Outcome>,
    /// Priority battles for the client, as lines ready to send.
    pub priority: Vec<String>,
}

/// One uploader's contribution to a rumble.
#[derive(Debug, Clone)]
pub struct UploaderStats {
    pub name: String,
    pub battles: u64,
    pub last_ms: i64,
}

/// A rumble as the pages see it.
#[derive(Debug, Clone)]
pub struct RumbleView {
    pub config: RumbleConfig,
    pub standings: Arc<Standings>,
    pub participants: Arc<Vec<Participant>>,
    pub total_battles: u64,
    pub last_battle_ms: i64,
    pub uploaders: Vec<UploaderStats>,
}

/// Everything the pages can show, at one moment.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub rumbles: Vec<RumbleView>,
    pub logos: Arc<Logos>,
}

impl Snapshot {
    pub fn rumble(&self, name: &str) -> Option<&RumbleView> {
        self.rumbles.iter().find(|rumble| rumble.config.name == name)
    }
}

struct Job {
    lines: Vec<String>,
    reply: oneshot::Sender<Reply>,
}

/// The handle that web handlers use to reach the engine thread.
pub struct Engine {
    jobs: SyncSender<Job>,
    snapshot: Arc<RwLock<Arc<Snapshot>>>,
}

impl Engine {
    /// Opens the database, rebuilds the scores from it and starts the thread.
    pub fn start(config: Config) -> Result<Engine, String> {
        let worker = Worker::new(config)?;
        let snapshot = worker.snapshot.clone();
        let (jobs, queue) = sync_channel(QUEUE_LENGTH);
        std::thread::Builder::new()
            .name("engine".to_string())
            .spawn(move || run_or_exit(worker, queue))
            .map_err(|e| format!("cannot start the engine thread: {e}"))?;
        Ok(Engine { jobs, snapshot })
    }

    /// Stores results, one per line, and waits until they are safe.
    pub async fn upload(&self, lines: Vec<String>) -> Reply {
        let count = lines.len();
        let busy = || Reply {
            outcomes: vec![Outcome::Retry("server busy".to_string()); count],
            priority: Vec::new(),
        };
        let (reply, answer) = oneshot::channel();
        if self.jobs.try_send(Job { lines, reply }).is_err() {
            return busy();
        }
        answer.await.unwrap_or_else(|_| busy())
    }

    /// The latest published snapshot.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.read().unwrap().clone()
    }
}

/// Stops the whole process if the thread panics, so that systemd can restart a working server.
fn run_or_exit(mut worker: Worker, queue: Receiver<Job>) {
    let run = std::panic::AssertUnwindSafe(|| worker.run(queue));
    if std::panic::catch_unwind(run).is_err() {
        eprintln!("engine: the thread panicked, stopping");
        std::process::exit(1);
    }
}

/// Decides whether a result may be stored in a rumble. The participant list is checked separately.
pub fn check_upload(
    upload: &Upload,
    rumble: &RumbleConfig,
    allowed_clients: &[String],
    now_ms: i64,
    max_age_ms: i64,
) -> Result<(), String> {
    if upload.version != "1" {
        return Err(format!("upload version {} is not supported", upload.version));
    }
    if !allowed_clients.contains(&upload.client) {
        return Err(format!(
            "client {} is not supported, use one of {allowed_clients:?}",
            upload.client
        ));
    }
    if upload.rounds != rumble.rounds {
        return Err(format!("this rumble has {} rounds per battle", rumble.rounds));
    }
    if upload.field != rumble.field {
        return Err(format!("this rumble is played on {}", rumble.field));
    }
    if upload.melee != rumble.is_melee() || upload.teams {
        return Err("wrong MELEE or TEAMS setting for this rumble".to_string());
    }
    if upload.hide_names != rumble.hide_names {
        let setting = if rumble.hide_names { "YES" } else { "NOT" };
        return Err(format!("this rumble needs HIDEENEMYNAMES={setting}"));
    }
    if upload.time_ms < now_ms - max_age_ms {
        return Err("the battle is too old".to_string());
    }
    if upload.time_ms > now_ms + MAX_FUTURE_MS {
        return Err("the battle is in the future, check your clock".to_string());
    }
    if upload.first.name == upload.second.name {
        return Err("a robot cannot fight itself".to_string());
    }
    for side in [&upload.first, &upload.second] {
        let bad_count = side.firsts < 0 || side.firsts > i64::from(rumble.rounds);
        if side.score < 0 || side.bullet_damage < 0 || bad_count {
            return Err(format!("impossible numbers for {}", side.name));
        }
    }
    Ok(())
}

/// A rumble's state on the engine thread.
struct Rumble {
    id: u32,
    config: RumbleConfig,
    participants: Arc<Vec<Participant>>,
    entrants: Vec<Entrant>,
    ids: HashMap<String, BotId>,
    tally: Tally,
    total_battles: u64,
    last_battle_ms: i64,
    uploaders: BTreeMap<String, UploaderStats>,
    standings: Arc<Standings>,
    /// The priority battles still to hand out, as lines ready to send.
    priority: Vec<String>,
    /// Whether the standings are out of date.
    dirty: bool,
}

impl Rumble {
    fn add_battle(&mut self, scores: &BattleScores, uploader: &str) {
        self.tally.add(scores, self.config.rounds);
        self.total_battles += 1;
        self.last_battle_ms = self.last_battle_ms.max(scores.time_ms);
        let stats = self.uploaders.entry(uploader.to_string()).or_insert_with(|| UploaderStats {
            name: uploader.to_string(),
            battles: 0,
            last_ms: 0,
        });
        stats.battles += 1;
        stats.last_ms = stats.last_ms.max(scores.time_ms);
        self.dirty = true;
    }
}

/// A result that passed the checks and waits to be stored.
struct Accepted {
    job: usize,
    line: usize,
    rumble: usize,
    uploader: String,
}

struct Worker {
    config: Config,
    store: Store,
    rumbles: Vec<Rumble>,
    bot_ids: HashMap<String, BotId>,
    uploader_ids: HashMap<String, u32>,
    logos: Arc<Logos>,
    snapshot: Arc<RwLock<Arc<Snapshot>>>,
    lists_read: Instant,
    standings_computed: Instant,
    rng: Rng,
}

impl Worker {
    fn new(config: Config) -> Result<Worker, String> {
        let store = open_store(&config.database)?;
        let database_error = |e: rusqlite::Error| format!("database error: {e}");

        let bot_ids = store
            .all_names(Names::Bot)
            .map_err(database_error)?
            .into_iter()
            .map(|(id, name)| (name, id))
            .collect();
        let uploaders = store.all_names(Names::Uploader).map_err(database_error)?;
        let uploader_names: HashMap<u32, String> = uploaders.iter().cloned().collect();
        let uploader_ids = uploaders.into_iter().map(|(id, name)| (name, id)).collect();

        let mut rumbles = Vec::new();
        for rumble in &config.rumbles {
            rumbles.push(Rumble {
                id: store.intern(Names::Rumble, &rumble.name).map_err(database_error)?,
                config: rumble.clone(),
                participants: Arc::new(Vec::new()),
                entrants: Vec::new(),
                ids: HashMap::new(),
                tally: Tally::default(),
                total_battles: 0,
                last_battle_ms: 0,
                uploaders: BTreeMap::new(),
                standings: Arc::new(Standings::default()),
                priority: Vec::new(),
                dirty: true,
            });
        }

        let started = Instant::now();
        let mut count = 0_u64;
        store
            .for_each_battle(|rumble_id, scores, uploader_id| {
                if let Some(rumble) = rumbles.iter_mut().find(|rumble| rumble.id == rumble_id) {
                    let uploader = uploader_names.get(&uploader_id).map_or("", String::as_str);
                    rumble.add_battle(scores, uploader);
                    count += 1;
                }
            })
            .map_err(database_error)?;
        eprintln!("engine: read {count} battles in {:.1?}", started.elapsed());

        let mut worker = Worker {
            config,
            store,
            rumbles,
            bot_ids,
            uploader_ids,
            logos: Arc::new(Logos::new()),
            snapshot: Arc::new(RwLock::new(Arc::new(Snapshot::default()))),
            lists_read: Instant::now(),
            standings_computed: Instant::now(),
            rng: Rng::new(),
        };
        worker.read_lists();
        worker.compute_standings();
        worker.publish();
        Ok(worker)
    }

    fn run(&mut self, queue: Receiver<Job>) {
        loop {
            match queue.recv_timeout(TICK) {
                Ok(first) => {
                    let mut jobs = vec![first];
                    jobs.extend(queue.try_iter().take(MAX_JOBS_PER_COMMIT - 1));
                    self.process(jobs);
                }
                Err(RecvTimeoutError::Timeout) => self.maintain(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Stores a group of jobs in one transaction and answers them.
    fn process(&mut self, jobs: Vec<Job>) {
        let now_ms = time::now_ms();
        let mut replies = Vec::with_capacity(jobs.len());
        let mut rows = Vec::new();
        let mut accepted = Vec::new();
        for (job, work) in jobs.iter().enumerate() {
            let mut outcomes = Vec::with_capacity(work.lines.len());
            for (line, text) in work.lines.iter().enumerate() {
                match self.prepare(text, now_ms) {
                    Ok((row, rumble, uploader)) => {
                        rows.push(row);
                        accepted.push(Accepted { job, line, rumble, uploader });
                        // Replaced below if the transaction commits.
                        outcomes.push(Outcome::Retry("database error".to_string()));
                    }
                    Err(outcome) => outcomes.push(outcome),
                }
            }
            replies.push(Reply { outcomes, priority: Vec::new() });
        }

        match self.store.insert_battles(&rows) {
            Ok(new) => {
                for ((row, place), is_new) in rows.iter().zip(&accepted).zip(new) {
                    let reply: &mut Reply = &mut replies[place.job];
                    reply.outcomes[place.line] = if is_new {
                        self.rumbles[place.rumble].add_battle(&row.scores, &place.uploader);
                        reply.priority.extend(self.next_priority_battle(place.rumble));
                        Outcome::Stored
                    } else {
                        Outcome::Duplicate
                    };
                }
            }
            Err(e) => eprintln!("engine: cannot store {} results: {e}", rows.len()),
        }

        self.maintain();

        for (job, reply) in jobs.into_iter().zip(replies) {
            // The client may have gone away, and then nobody needs the answer.
            let _ = job.reply.send(reply);
        }
    }

    /// Parses and checks one result. Returns the row to store, the rumble and the uploader's name,
    /// or the reason it cannot be stored.
    fn prepare(&mut self, text: &str, now_ms: i64) -> Result<(BattleRow, usize, String), Outcome> {
        let upload = protocol::parse_upload(text).map_err(Outcome::Rejected)?;
        let rumble_index = self
            .rumbles
            .iter()
            .position(|rumble| rumble.config.name == upload.game)
            .ok_or_else(|| Outcome::Rejected(format!("there is no rumble {}", upload.game)))?;
        let rumble = &self.rumbles[rumble_index];
        let max_age_ms = self.config.max_battle_age_hours as i64 * 3_600_000;
        check_upload(&upload, &rumble.config, &self.config.allowed_clients, now_ms, max_age_ms)
            .map_err(Outcome::Rejected)?;

        let listed_id = |name: &str| {
            rumble.ids.get(name).copied().ok_or_else(|| {
                Outcome::Rejected(format!("{name} is not a participant of {}", upload.game))
            })
        };
        let first_id = listed_id(&upload.first.name)?;
        let second_id = listed_id(&upload.second.name)?;
        let rumble_id = rumble.id;

        let uploader: String = upload.user.chars().take(MAX_USER_CHARS).collect();
        let uploader_id = self.uploader_id(&uploader)?;

        let (lo, hi, lo_id, hi_id) = if first_id < second_id {
            (&upload.first, &upload.second, first_id, second_id)
        } else {
            (&upload.second, &upload.first, second_id, first_id)
        };
        let row = BattleRow {
            rumble: rumble_id,
            scores: BattleScores {
                lo: lo_id,
                hi: hi_id,
                lo_score: lo.score,
                hi_score: hi.score,
                lo_firsts: lo.firsts,
                hi_firsts: hi.firsts,
                time_ms: upload.time_ms,
            },
            lo_bullet_damage: lo.bullet_damage,
            hi_bullet_damage: hi.bullet_damage,
            uploader: uploader_id,
            client: upload.client.clone(),
            received_ms: now_ms,
        };
        Ok((row, rumble_index, uploader))
    }

    fn uploader_id(&mut self, name: &str) -> Result<u32, Outcome> {
        if let Some(id) = self.uploader_ids.get(name) {
            return Ok(*id);
        }
        let id = self.store.intern(Names::Uploader, name).map_err(|e| {
            eprintln!("engine: cannot add uploader {name}: {e}");
            Outcome::Retry("database error".to_string())
        })?;
        self.uploader_ids.insert(name.to_string(), id);
        Ok(id)
    }

    /// Hands out a priority battle, about once per battle fought.
    fn next_priority_battle(&mut self, rumble: usize) -> Option<String> {
        let rumble = &mut self.rumbles[rumble];
        if self.rng.f64() < priority::chance_per_result(rumble.config.melee_bots) {
            rumble.priority.pop()
        } else {
            None
        }
    }

    /// Does whatever the timers say is due.
    fn maintain(&mut self) {
        let mut changed = false;
        if self.lists_read.elapsed().as_secs() >= self.config.participants_seconds {
            self.read_lists();
            changed = true;
        }
        if self.standings_computed.elapsed().as_secs() >= self.config.standings_seconds {
            changed |= self.compute_standings();
        }
        if changed {
            self.publish();
        }
    }

    /// Reads the participant lists and logos again. When a list cannot be read, or is suddenly
    /// empty, the rumble keeps the list it has, so a bad fetch never retires anyone.
    fn read_lists(&mut self) {
        self.lists_read = Instant::now();
        self.logos = Arc::new(logos::load_dir(&self.config.logos));
        for index in 0..self.rumbles.len() {
            let name = self.rumbles[index].config.name.clone();
            let text = match self.rumbles[index].config.participants.fetch() {
                Ok(text) => text,
                Err(reason) => {
                    eprintln!("engine: keeping the current list of {name}: {reason}");
                    continue;
                }
            };
            let list = participants::parse_list(&text);
            if list.participants == *self.rumbles[index].participants {
                continue;
            }
            for line in &list.invalid {
                eprintln!("engine: ignoring a line in the list of {name}: {line}");
            }
            if list.participants.is_empty() {
                eprintln!("engine: keeping the current list of {name}: the new one is empty");
                continue;
            }
            if let Err(e) = self.set_participants(index, list.participants) {
                eprintln!("engine: keeping the current list of {name}: {e}");
            }
        }
    }

    fn set_participants(&mut self, index: usize, list: Vec<Participant>) -> rusqlite::Result<()> {
        let mut entrants = Vec::with_capacity(list.len());
        for participant in &list {
            let id = match self.bot_ids.get(&participant.name) {
                Some(id) => *id,
                None => self.store.intern(Names::Bot, &participant.name)?,
            };
            self.bot_ids.insert(participant.name.clone(), id);
            entrants.push(Entrant { id, name: participant.name.clone() });
        }
        let rumble = &mut self.rumbles[index];
        eprintln!("engine: {} now has {} participants", rumble.config.name, list.len());
        rumble.ids = entrants.iter().map(|bot| (bot.name.clone(), bot.id)).collect();
        rumble.entrants = entrants;
        rumble.participants = Arc::new(list);
        rumble.dirty = true;
        Ok(())
    }

    /// Recomputes the standings and priority battles of every rumble with changes, and returns
    /// whether there were any.
    fn compute_standings(&mut self) -> bool {
        self.standings_computed = Instant::now();
        let mut changed = false;
        for rumble in self.rumbles.iter_mut().filter(|rumble| rumble.dirty) {
            rumble.standings = Arc::new(Standings::compute(&rumble.entrants, &rumble.tally));
            let names: HashMap<BotId, &str> =
                rumble.entrants.iter().map(|bot| (bot.id, bot.name.as_str())).collect();
            rumble.priority = priority::next_battles(
                &rumble.entrants,
                &rumble.tally,
                PRIORITY_QUEUE_LENGTH,
                &mut self.rng,
            )
            .into_iter()
            .map(|(first, second)| protocol::priority_line(names[&first], names[&second]))
            .collect();
            rumble.dirty = false;
            changed = true;
        }
        changed
    }

    fn publish(&self) {
        let rumbles = self
            .rumbles
            .iter()
            .map(|rumble| RumbleView {
                config: rumble.config.clone(),
                standings: rumble.standings.clone(),
                participants: rumble.participants.clone(),
                total_battles: rumble.total_battles,
                last_battle_ms: rumble.last_battle_ms,
                uploaders: rumble.uploaders.values().cloned().collect(),
            })
            .collect();
        let snapshot = Snapshot { rumbles, logos: self.logos.clone() };
        *self.snapshot.write().unwrap() = Arc::new(snapshot);
    }
}

fn open_store(path: &Path) -> Result<Store, String> {
    if let Some(directory) = path.parent().filter(|directory| !directory.as_os_str().is_empty()) {
        std::fs::create_dir_all(directory)
            .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
    }
    Store::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::participants::Source;
    use crate::protocol::Side;

    const NOW: i64 = 1_000_000_000_000;
    const DAY: i64 = 86_400_000;

    fn rumble() -> RumbleConfig {
        RumbleConfig {
            name: "autorumble".to_string(),
            title: "AutoRumble".to_string(),
            field: "800x600".to_string(),
            rounds: 35,
            melee_bots: None,
            hide_names: true,
            participants: Source::File("unused".into()),
        }
    }

    fn side(name: &str) -> Side {
        Side { name: name.to_string(), score: 100, bullet_damage: 50, firsts: 10 }
    }

    fn upload() -> Upload {
        Upload {
            game: "autorumble".to_string(),
            version: "1".to_string(),
            client: "1.11.2".to_string(),
            teams: false,
            melee: false,
            hide_names: true,
            rounds: 35,
            field: "800x600".to_string(),
            user: "Tester".to_string(),
            time_ms: NOW,
            first: side("a.One 1"),
            second: side("b.Two 1"),
        }
    }

    fn check(upload: &Upload) -> Result<(), String> {
        check_upload(upload, &rumble(), &["1.11.2".to_string()], NOW, DAY)
    }

    #[test]
    fn a_good_upload_passes() {
        assert_eq!(check(&upload()), Ok(()));
    }

    #[test]
    fn the_rumble_settings_must_match() {
        let changes: [fn(&mut Upload); 7] = [
            |u| u.version = "2".to_string(),
            |u| u.client = "1.0".to_string(),
            |u| u.rounds = 10,
            |u| u.field = "1000x1000".to_string(),
            |u| u.melee = true,
            |u| u.teams = true,
            |u| u.hide_names = false,
        ];
        for change in changes {
            let mut bad = upload();
            change(&mut bad);
            assert!(check(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn old_and_future_battles_are_rejected() {
        let mut bad = upload();
        bad.time_ms = NOW - DAY - 1;
        assert!(check(&bad).is_err());
        bad.time_ms = NOW + MAX_FUTURE_MS + 1;
        assert!(check(&bad).is_err());
        bad.time_ms = NOW - DAY;
        assert!(check(&bad).is_ok());
    }

    #[test]
    fn impossible_numbers_are_rejected() {
        let changes: [fn(&mut Upload); 4] = [
            |u| u.second.name = u.first.name.clone(),
            |u| u.first.score = -1,
            |u| u.first.firsts = 36,
            |u| u.second.bullet_damage = -1,
        ];
        for change in changes {
            let mut bad = upload();
            change(&mut bad);
            assert!(check(&bad).is_err(), "{bad:?}");
        }
    }
}
