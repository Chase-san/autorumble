//! Times the scoring for a rumble of a given size, without the server or the database.
//!
//! `cargo run --release --example scale -- ROBOTS BATTLES_PER_PAIRING`
//!
//! Standings are computed from each pairing's totals, so the time should depend on the number of
//! robots and not on the number of battles. This shows whether that holds.

use std::time::Instant;

use autorumble::priority;
use autorumble::scoring::{BattleScores, BotId, Entrant, Standings, Tally};
use fastrand::Rng;

fn main() {
    let number = |index: usize| -> u64 {
        std::env::args()
            .nth(index)
            .and_then(|arg| arg.parse().ok())
            .expect("usage: scale ROBOTS BATTLES_PER_PAIRING")
    };
    let (robots, per_pairing) = (number(1) as u32, number(2));
    let mut rng = Rng::with_seed(1);

    let entrants: Vec<Entrant> =
        (1..=robots).map(|id| Entrant { id, name: format!("load.Robot{id} 1.0") }).collect();

    let started = Instant::now();
    let mut tally = Tally::default();
    let mut battles = 0_u64;
    for lo in 1..=robots {
        for hi in lo + 1..=robots {
            for _ in 0..per_pairing {
                let score = rng.i64(0..=5000);
                let battle = BattleScores {
                    lo: lo as BotId,
                    hi: hi as BotId,
                    lo_score: score,
                    hi_score: 5000 - score,
                    lo_firsts: 17,
                    hi_firsts: 18,
                    time_ms: 1,
                };
                tally.add(&battle, 35);
                battles += 1;
            }
        }
    }
    let adding = started.elapsed();

    let started = Instant::now();
    let standings = Standings::compute(&entrants, &tally);
    let computing = started.elapsed();

    let started = Instant::now();
    let chosen = priority::next_battles(&entrants, &tally, 1000, &mut rng);
    let choosing = started.elapsed();

    let peak = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            let line = status.lines().find(|line| line.starts_with("VmHWM"))?.to_string();
            line.split_whitespace().nth(1)?.parse::<u64>().ok()
        })
        .map_or(0, |kilobytes| kilobytes / 1024);

    println!(
        "{robots:5} robots {battles:>11} battles | adding {:7.0} ms | standings {:7.1} ms | priority {:6.1} ms | peak memory {peak:5} MB | top APS {:.2}, {} chosen",
        adding.as_secs_f64() * 1000.0,
        computing.as_secs_f64() * 1000.0,
        choosing.as_secs_f64() * 1000.0,
        standings.bots[0].aps,
        chosen.len(),
    );
}
