//! Chooses priority battles, which the server hands to clients to make the scores settle faster.
//!
//! Pairings that have never been fought come first. After that come the pairings where one more
//! battle would shrink the uncertainty in the scores the most.

use fastrand::Rng;

use crate::scoring::{BotId, Entrant, Tally};

/// How much more a pairing is worth when it is not yet clear which robot wins it. Which robot wins
/// a pairing decides PWIN, so these pairings matter more than their variance alone says.
const UNDECIDED_WEIGHT: f64 = 10.0;

/// Returns up to `limit` pairings in random order, so clients do not all run the same battles.
pub fn next_battles(
    entrants: &[Entrant],
    tally: &Tally,
    limit: usize,
    rng: &mut Rng,
) -> Vec<(BotId, BotId)> {
    let mut missing = Vec::new();
    let mut fought = Vec::new();
    for (i, bot) in entrants.iter().enumerate() {
        for opponent in &entrants[i + 1..] {
            match tally.pair(bot.id, opponent.id) {
                None => missing.push((bot.id, opponent.id)),
                Some(pair) => {
                    let battles = f64::from(pair.battles);
                    // How much one more battle shrinks the variance of this pairing's mean.
                    let mut gain = pair.variance / (battles * (battles + 1.0));
                    if (pair.aps - 50.0).abs() < pair.ci() {
                        gain *= UNDECIDED_WEIGHT;
                    }
                    fought.push((gain, (bot.id, opponent.id)));
                }
            }
        }
    }

    let mut chosen: Vec<(BotId, BotId)> = if missing.is_empty() {
        fought.sort_by(|a, b| b.0.total_cmp(&a.0));
        fought.truncate(limit);
        fought.into_iter().map(|(_, pair)| pair).collect()
    } else {
        missing
    };
    rng.shuffle(&mut chosen);
    chosen.truncate(limit);
    chosen
}

/// The chance that a result gets a priority battle, so that each battle gets about one.
/// A melee battle uploads one result per pair of robots in it.
pub fn chance_per_result(melee_bots: Option<u32>) -> f64 {
    match melee_bots {
        Some(bots) => 2.0 / f64::from(bots * (bots - 1)),
        None => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::BattleScores;

    fn entrants(count: u32) -> Vec<Entrant> {
        (1..=count).map(|id| Entrant { id, name: format!("bot{id}") }).collect()
    }

    fn fight(tally: &mut Tally, lo: BotId, hi: BotId, lo_score: i64, times: u32) {
        let battle = BattleScores {
            lo,
            hi,
            lo_score,
            hi_score: 100 - lo_score,
            lo_firsts: 0,
            hi_firsts: 0,
            time_ms: 1,
        };
        for _ in 0..times {
            tally.add(&battle, 35);
        }
    }

    #[test]
    fn missing_pairings_come_first() {
        let mut tally = Tally::default();
        fight(&mut tally, 1, 2, 60, 1);
        let battles = next_battles(&entrants(3), &tally, 10, &mut Rng::with_seed(1));
        assert_eq!(battles.len(), 2);
        assert!(battles.iter().all(|pair| *pair == (1, 3) || *pair == (2, 3)));
    }

    #[test]
    fn then_the_least_certain_pairings() {
        let mut tally = Tally::default();
        fight(&mut tally, 1, 2, 90, 100);
        fight(&mut tally, 1, 3, 90, 2);
        fight(&mut tally, 2, 3, 90, 100);
        let battles = next_battles(&entrants(3), &tally, 1, &mut Rng::with_seed(1));
        assert_eq!(battles, [(1, 3)]);
    }

    #[test]
    fn an_undecided_pairing_beats_a_decided_one() {
        let mut tally = Tally::default();
        fight(&mut tally, 1, 2, 90, 10);
        fight(&mut tally, 1, 3, 51, 10);
        fight(&mut tally, 2, 3, 90, 10);
        let battles = next_battles(&entrants(3), &tally, 1, &mut Rng::with_seed(1));
        assert_eq!(battles, [(1, 3)]);
    }

    #[test]
    fn respects_the_limit() {
        let battles = next_battles(&entrants(10), &Tally::default(), 7, &mut Rng::with_seed(1));
        assert_eq!(battles.len(), 7);
    }

    #[test]
    fn nothing_to_do_with_one_robot() {
        assert!(
            next_battles(&entrants(1), &Tally::default(), 7, &mut Rng::with_seed(1)).is_empty()
        );
    }

    #[test]
    fn about_one_priority_battle_per_battle() {
        assert_eq!(chance_per_result(None), 1.0);
        assert_eq!(chance_per_result(Some(6)), 1.0 / 15.0);
    }
}
