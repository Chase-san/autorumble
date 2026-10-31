//! Works out all the scores from the battle results. `docs/scoring.html` defines each score.
//!
//! [`Tally`] adds up the battles of each pairing. Its totals do not depend on the order of the
//! battles, so replaying the stored battles always gives the same scores.
//! [`Standings::compute`] is a plain function from those totals to everything the pages show.

use std::collections::HashMap;

pub type BotId = u32;

/// The prior of a pairing's per-battle APS variance, so that a pairing with few battles never
/// looks more certain than it is. The same numbers as LiteRumble.
const PRIOR_VARIANCE: f64 = 16.0;
const PRIOR_BATTLES: f64 = 3.0;

/// The multiplier for a 95% confidence interval.
const CI_95: f64 = 1.96;

/// One battle's numbers for a pairing. `lo` always has the lower id of the two robots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleScores {
    pub lo: BotId,
    pub hi: BotId,
    pub lo_score: i64,
    pub hi_score: i64,
    pub lo_firsts: i64,
    pub hi_firsts: i64,
    pub time_ms: i64,
}

/// The totals of one pairing, kept from the side of the robot with the lower id.
#[derive(Debug, Clone, Copy)]
struct PairTotals {
    battles: u32,
    aps_sum: f64,
    aps_squares: f64,
    aps_min: f64,
    aps_max: f64,
    lo_survival_sum: f64,
    hi_survival_sum: f64,
    last_ms: i64,
}

/// A pairing's numbers from one robot's side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairSide {
    pub battles: u32,
    pub aps: f64,
    pub min_aps: f64,
    pub survival: f64,
    /// The per-battle variance of the APS, pulled towards the prior.
    pub variance: f64,
    pub last_ms: i64,
}

impl PairSide {
    /// The half width of the 95% confidence interval on this pairing's APS.
    pub fn ci(&self) -> f64 {
        CI_95 * (self.variance / f64::from(self.battles)).sqrt()
    }
}

/// The totals of every pairing in one rumble.
#[derive(Debug, Default)]
pub struct Tally {
    pairs: HashMap<(BotId, BotId), PairTotals>,
}

impl Tally {
    /// Adds one battle of a rumble with the given number of rounds.
    pub fn add(&mut self, battle: &BattleScores, rounds: u32) {
        let total_score = battle.lo_score + battle.hi_score;
        let aps = if total_score > 0 {
            100.0 * battle.lo_score as f64 / total_score as f64
        } else {
            50.0
        };
        let survival = |firsts: i64| 100.0 * firsts as f64 / f64::from(rounds);
        let pair = self.pairs.entry((battle.lo, battle.hi)).or_insert(PairTotals {
            battles: 0,
            aps_sum: 0.0,
            aps_squares: 0.0,
            aps_min: f64::INFINITY,
            aps_max: f64::NEG_INFINITY,
            lo_survival_sum: 0.0,
            hi_survival_sum: 0.0,
            last_ms: 0,
        });
        pair.battles += 1;
        pair.aps_sum += aps;
        pair.aps_squares += aps * aps;
        pair.aps_min = pair.aps_min.min(aps);
        pair.aps_max = pair.aps_max.max(aps);
        pair.lo_survival_sum += survival(battle.lo_firsts);
        pair.hi_survival_sum += survival(battle.hi_firsts);
        pair.last_ms = pair.last_ms.max(battle.time_ms);
    }

    /// The pairing of `bot` and `opponent` from the side of `bot`, if they have fought.
    pub fn pair(&self, bot: BotId, opponent: BotId) -> Option<PairSide> {
        let key = if bot < opponent { (bot, opponent) } else { (opponent, bot) };
        let pair = self.pairs.get(&key)?;
        let battles = f64::from(pair.battles);
        let lo_aps = pair.aps_sum / battles;
        let raw_variance = (pair.aps_squares / battles - lo_aps * lo_aps).max(0.0);
        let evidence = battles - 1.0;
        let variance =
            (PRIOR_BATTLES * PRIOR_VARIANCE + evidence * raw_variance) / (PRIOR_BATTLES + evidence);
        let (aps, min_aps, survival_sum) = if bot < opponent {
            (lo_aps, pair.aps_min, pair.lo_survival_sum)
        } else {
            (100.0 - lo_aps, 100.0 - pair.aps_max, pair.hi_survival_sum)
        };
        Some(PairSide {
            battles: pair.battles,
            aps,
            min_aps,
            survival: survival_sum / battles,
            variance,
            last_ms: pair.last_ms,
        })
    }
}

/// A robot on a rumble's participant list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entrant {
    pub id: BotId,
    pub name: String,
}

/// A robot's scores.
#[derive(Debug, Clone)]
pub struct BotScore {
    pub name: String,
    pub aps: f64,
    /// The half width of the 95% confidence interval on the APS, or -1 with no battles.
    pub aps_ci: f64,
    pub pwin: f64,
    pub anpp: f64,
    pub vote: f64,
    pub survival: f64,
    pub pairings: u32,
    pub battles: u64,
    pub last_ms: i64,
}

/// One pairing as shown on a robot's page.
#[derive(Debug, Clone)]
pub struct PairingView {
    /// The opponent, as an index into [`Standings::bots`].
    pub opponent: usize,
    pub battles: u32,
    pub aps: f64,
    pub aps_ci: f64,
    pub min_aps: f64,
    pub survival: f64,
    pub npp: f64,
    pub knn_pbi: f64,
    pub last_ms: i64,
}

/// Everything the pages show about one rumble, computed at one moment.
#[derive(Debug, Default)]
pub struct Standings {
    /// Sorted by APS, best first. Robots without battles come last.
    pub bots: Vec<BotScore>,
    /// The pairings of each robot in `bots`.
    pub pairings: Vec<Vec<PairingView>>,
    pub by_name: HashMap<String, usize>,
    /// Whether every robot has fought every other robot.
    pub stable: bool,
}

impl Standings {
    /// Computes the standings of the listed robots. Pairings with robots no longer on the list do
    /// not count, but stay in the tally in case those robots come back.
    pub fn compute(entrants: &[Entrant], tally: &Tally) -> Standings {
        let ranked = rank_by_aps(entrants, tally);
        let count = ranked.len();

        // sides[i * count + j] is the pairing of robot i and robot j from the side of robot i.
        let mut sides = vec![None; count * count];
        for (i, bot) in ranked.iter().enumerate() {
            for (j, opponent) in ranked.iter().enumerate() {
                if i != j {
                    sides[i * count + j] = tally.pair(bot.id, opponent.id);
                }
            }
        }
        let side = |i: usize, j: usize| -> Option<&PairSide> { sides[i * count + j].as_ref() };
        let aps = |i: usize, j: usize| side(i, j).map(|pair| pair.aps);

        let votes = votes(count, &aps);
        let npp = normalised_pairings(count, &aps);
        let knn_pbi = knn_problem_bot_index(count, &aps);

        let mut standings = Standings::default();
        for (i, bot) in ranked.iter().enumerate() {
            let mut pairings = Vec::new();
            let (mut aps_sum, mut survival_sum, mut npp_sum, mut error_sum) = (0.0, 0.0, 0.0, 0.0);
            let mut win_margin = 0_i64;
            let mut battles = 0_u64;
            let mut last_ms = 0;
            for j in 0..count {
                let Some(pair) = side(i, j) else { continue };
                aps_sum += pair.aps;
                survival_sum += pair.survival;
                npp_sum += npp[i * count + j];
                error_sum += pair.variance / f64::from(pair.battles);
                if pair.aps > 50.0 {
                    win_margin += 1;
                } else if pair.aps < 50.0 {
                    win_margin -= 1;
                }
                battles += u64::from(pair.battles);
                last_ms = last_ms.max(pair.last_ms);
                pairings.push(PairingView {
                    opponent: j,
                    battles: pair.battles,
                    aps: pair.aps,
                    aps_ci: pair.ci(),
                    min_aps: pair.min_aps,
                    survival: pair.survival,
                    npp: npp[i * count + j],
                    knn_pbi: knn_pbi[i * count + j],
                    last_ms: pair.last_ms,
                });
            }
            let played = pairings.len() as f64;
            standings.by_name.insert(bot.name.clone(), i);
            standings.bots.push(BotScore {
                name: bot.name.clone(),
                aps: aps_sum / played,
                aps_ci: CI_95 * error_sum.sqrt() / played,
                pwin: 50.0 * win_margin as f64 / played + 50.0,
                anpp: npp_sum / played,
                vote: 100.0 * votes[i] / played,
                survival: survival_sum / played,
                pairings: pairings.len() as u32,
                battles,
                last_ms,
            });
            standings.pairings.push(pairings);
        }

        for bot in entrants.iter().filter(|bot| !ranked.iter().any(|other| other.id == bot.id)) {
            standings.by_name.insert(bot.name.clone(), standings.bots.len());
            standings.bots.push(BotScore {
                name: bot.name.clone(),
                aps: 0.0,
                aps_ci: -1.0,
                pwin: 0.0,
                anpp: 0.0,
                vote: 0.0,
                survival: 0.0,
                pairings: 0,
                battles: 0,
                last_ms: 0,
            });
            standings.pairings.push(Vec::new());
        }

        let everyone = entrants.len().saturating_sub(1) as u32;
        standings.stable =
            everyone > 0 && standings.bots.iter().all(|bot| bot.pairings == everyone);
        standings
    }
}

/// The robots with at least one pairing, best APS first.
fn rank_by_aps<'a>(entrants: &'a [Entrant], tally: &Tally) -> Vec<&'a Entrant> {
    let mut scored: Vec<(f64, &Entrant)> = Vec::with_capacity(entrants.len());
    for bot in entrants {
        let (mut sum, mut played) = (0.0, 0_u32);
        for opponent in entrants {
            if opponent.id == bot.id {
                continue;
            }
            if let Some(pair) = tally.pair(bot.id, opponent.id) {
                sum += pair.aps;
                played += 1;
            }
        }
        if played > 0 {
            scored.push((sum / f64::from(played), bot));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, bot)| bot).collect()
}

/// Each robot gives one vote to the opponent that scores best against it, split on a tie.
fn votes(count: usize, aps: &impl Fn(usize, usize) -> Option<f64>) -> Vec<f64> {
    let mut votes = vec![0.0; count];
    for target in 0..count {
        let scores = (0..count).filter_map(|attacker| aps(attacker, target));
        let best = scores.fold(f64::NEG_INFINITY, f64::max);
        let winners: Vec<usize> =
            (0..count).filter(|&attacker| aps(attacker, target) == Some(best)).collect();
        for winner in &winners {
            votes[*winner] += 1.0 / winners.len() as f64;
        }
    }
    votes
}

/// NPP: each robot's score against a target, scaled so that the worst score against that target
/// is 0 and the best is 100. Returned as `npp[robot * count + target]`.
fn normalised_pairings(count: usize, aps: &impl Fn(usize, usize) -> Option<f64>) -> Vec<f64> {
    let mut npp = vec![0.0; count * count];
    for target in 0..count {
        let scores = || (0..count).filter_map(|robot| aps(robot, target));
        let lowest = scores().fold(f64::INFINITY, f64::min);
        let highest = scores().fold(f64::NEG_INFINITY, f64::max);
        for robot in 0..count {
            if let Some(score) = aps(robot, target) {
                npp[robot * count + target] = if highest > lowest {
                    100.0 * (score - lowest) / (highest - lowest)
                } else {
                    100.0
                };
            }
        }
    }
    npp
}

/// KNNPBI: how much better a robot does against a target than its neighbours in the rankings do.
/// Returned as `pbi[robot * count + target]`.
///
/// The neighbours are `half_k` places above the robot and `half_k - 1` below, as in LiteRumble.
fn knn_problem_bot_index(count: usize, aps: &impl Fn(usize, usize) -> Option<f64>) -> Vec<f64> {
    let half_k = ((count as f64).sqrt() / 2.0).ceil() as usize;
    let mut pbi = vec![0.0; count * count];
    for robot in 0..count {
        let low = robot.saturating_sub(half_k);
        let high = (robot + half_k).min(count.saturating_sub(1));
        let above_end = (robot + 1).min(high);
        for target in 0..count {
            let Some(score) = aps(robot, target) else { continue };
            let neighbours = (low..robot).chain(above_end..high);
            let (mut sum, mut seen) = (0.0, 0_u32);
            for neighbour in neighbours {
                if let Some(other) = aps(neighbour, target) {
                    sum += other;
                    seen += 1;
                }
            }
            if seen > 0 {
                pbi[robot * count + target] = score - sum / f64::from(seen);
            }
        }
    }
    pbi
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entrants(names: &[&str]) -> Vec<Entrant> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| Entrant { id: i as BotId + 1, name: name.to_string() })
            .collect()
    }

    fn battle(lo: BotId, hi: BotId, lo_score: i64, hi_score: i64) -> BattleScores {
        BattleScores { lo, hi, lo_score, hi_score, lo_firsts: 20, hi_firsts: 15, time_ms: 1000 }
    }

    /// A beats B 60-40, A beats C 70-30 and B beats C 55-45.
    fn three_bots() -> (Vec<Entrant>, Tally) {
        let mut tally = Tally::default();
        tally.add(&battle(1, 2, 60, 40), 35);
        tally.add(&battle(1, 3, 70, 30), 35);
        tally.add(&battle(2, 3, 55, 45), 35);
        (entrants(&["A", "B", "C"]), tally)
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} is not {expected}");
    }

    #[test]
    fn scores_three_bots_by_hand() {
        let (bots, tally) = three_bots();
        let standings = Standings::compute(&bots, &tally);

        let names: Vec<&str> = standings.bots.iter().map(|bot| bot.name.as_str()).collect();
        assert_eq!(names, ["A", "B", "C"]);
        assert!(standings.stable);

        let [a, b, c] = &standings.bots[..] else { panic!() };
        assert_close(a.aps, 65.0);
        assert_close(b.aps, 47.5);
        assert_close(c.aps, 37.5);

        assert_close(a.pwin, 100.0);
        assert_close(b.pwin, 50.0);
        assert_close(c.pwin, 0.0);

        // A has the best score against B and C, and B has the best score against A.
        assert_close(a.vote, 100.0);
        assert_close(b.vote, 50.0);
        assert_close(c.vote, 0.0);

        // A is always best, C is always worst, and B is best against A but worst against C.
        assert_close(a.anpp, 100.0);
        assert_close(b.anpp, 50.0);
        assert_close(c.anpp, 0.0);

        assert_eq!(a.pairings, 2);
        assert_eq!(a.battles, 2);
    }

    #[test]
    fn pairings_read_from_both_sides() {
        let (_, tally) = three_bots();
        assert_close(tally.pair(1, 2).unwrap().aps, 60.0);
        assert_close(tally.pair(2, 1).unwrap().aps, 40.0);
        assert_close(tally.pair(1, 2).unwrap().survival, 100.0 * 20.0 / 35.0);
        assert_close(tally.pair(2, 1).unwrap().survival, 100.0 * 15.0 / 35.0);
        assert!(tally.pair(1, 9).is_none());
    }

    #[test]
    fn min_aps_is_the_worst_battle_of_each_side() {
        let mut tally = Tally::default();
        tally.add(&battle(1, 2, 60, 40), 35);
        tally.add(&battle(1, 2, 80, 20), 35);
        assert_close(tally.pair(1, 2).unwrap().min_aps, 60.0);
        assert_close(tally.pair(2, 1).unwrap().min_aps, 20.0);
    }

    #[test]
    fn a_battle_with_no_score_counts_as_even() {
        let mut tally = Tally::default();
        tally.add(&battle(1, 2, 0, 0), 35);
        assert_close(tally.pair(1, 2).unwrap().aps, 50.0);
    }

    #[test]
    fn the_order_of_battles_does_not_matter() {
        let battles = [battle(1, 2, 60, 40), battle(1, 2, 30, 70), battle(1, 2, 90, 10)];
        let (mut forward, mut backward) = (Tally::default(), Tally::default());
        for b in &battles {
            forward.add(b, 35);
        }
        for b in battles.iter().rev() {
            backward.add(b, 35);
        }
        assert_eq!(forward.pair(1, 2), backward.pair(1, 2));
    }

    #[test]
    fn variance_starts_at_the_prior_and_moves_to_the_data() {
        let mut tally = Tally::default();
        tally.add(&battle(1, 2, 50, 50), 35);
        assert_close(tally.pair(1, 2).unwrap().variance, PRIOR_VARIANCE);
        for _ in 0..999 {
            tally.add(&battle(1, 2, 50, 50), 35);
        }
        assert!(tally.pair(1, 2).unwrap().variance < 0.1);
    }

    #[test]
    fn robots_off_the_list_do_not_count() {
        let (_, tally) = three_bots();
        let standings = Standings::compute(&entrants(&["A", "B"]), &tally);
        assert_eq!(standings.bots.len(), 2);
        assert_close(standings.bots[0].aps, 60.0);
        assert_eq!(standings.bots[0].pairings, 1);
    }

    #[test]
    fn robots_without_battles_come_last() {
        let (mut bots, tally) = three_bots();
        bots.insert(0, Entrant { id: 9, name: "New".to_string() });
        let standings = Standings::compute(&bots, &tally);
        assert_eq!(standings.bots[3].name, "New");
        assert_eq!(standings.bots[3].pairings, 0);
        assert_eq!(standings.bots[3].aps_ci, -1.0);
        assert_eq!(standings.by_name["New"], 3);
        assert!(!standings.stable);
    }

    #[test]
    fn knn_pbi_compares_with_neighbours() {
        let (bots, tally) = three_bots();
        let standings = Standings::compute(&bots, &tally);
        // half_k is 1 with 3 robots. B's only neighbour is A, who scores 70 against C to B's 55.
        let b_vs_c = &standings.pairings[1].iter().find(|p| p.opponent == 2).unwrap();
        assert_close(b_vs_c.knn_pbi, 55.0 - 70.0);
    }

    /// A rumble with a missing pairing (C and F) and a pairing with several battles (A and B).
    /// The expected numbers come from LiteRumble's batch formulas, written out in numpy.
    #[test]
    fn matches_the_numpy_formulas() {
        const NAMES: [&str; 7] = ["A", "B", "C", "D", "E", "F", "G"];
        const PAIRS: [(BotId, BotId, &[i64]); 20] = [
            (1, 2, &[620, 580, 700]),
            (1, 3, &[710]),
            (1, 4, &[550]),
            (1, 5, &[820]),
            (1, 6, &[450]),
            (1, 7, &[900]),
            (2, 3, &[530]),
            (2, 4, &[410]),
            (2, 5, &[760]),
            (2, 6, &[650]),
            (2, 7, &[880]),
            (3, 4, &[480]),
            (3, 5, &[610]),
            (3, 7, &[740]),
            (4, 5, &[690]),
            (4, 6, &[570]),
            (4, 7, &[790]),
            (5, 6, &[430]),
            (5, 7, &[640]),
            (6, 7, &[720]),
        ];
        // (robot, APS, APS CI, PWIN, ANPP, Vote), best first.
        const ROBOTS: [(&str, f64, f64, f64, f64, f64); 7] = [
            ("A", 67.7222222222, 3.0385008216, 83.3333333333, 90.9909909910, 83.3333333333),
            ("D", 60.1666666667, 3.2006665972, 83.3333333333, 72.4873022699, 0.0),
            ("B", 59.9444444444, 3.0385008216, 66.6666666667, 76.2245004530, 16.6666666667),
            ("F", 52.4, 3.5061545887, 60.0, 57.1864963681, 20.0),
            ("C", 51.8, 3.5061545887, 40.0, 56.5250339317, 0.0),
            ("E", 36.5, 3.2006665972, 16.6666666667, 23.3325992150, 0.0),
            ("G", 22.1666666667, 3.2006665972, 0.0, 0.0, 0.0),
        ];
        // (robot, opponent, NPP, KNNPBI)
        const PAIRINGS: [(&str, &str, f64, f64); 8] = [
            ("A", "B", 100.0, 4.3333333333),
            ("A", "F", 45.9459459459, -12.0),
            ("D", "A", 77.7777777778, 8.3333333333),
            ("B", "C", 60.0, -8.5),
            ("F", "A", 100.0, 18.1111111111),
            ("C", "D", 79.4117647059, 9.6666666667),
            ("E", "C", 28.8888888889, 0.0),
            ("G", "E", 0.0, -25.0),
        ];

        let mut tally = Tally::default();
        for (lo, hi, scores) in PAIRS {
            for score in scores {
                tally.add(&battle(lo, hi, *score, 1000 - score), 35);
            }
        }
        let standings = Standings::compute(&entrants(&NAMES), &tally);

        let near = |actual: f64, expected: f64| {
            assert!((actual - expected).abs() < 1e-8, "{actual} is not {expected}");
        };
        for (bot, (name, aps, ci, pwin, anpp, vote)) in standings.bots.iter().zip(ROBOTS) {
            assert_eq!(bot.name, name);
            near(bot.aps, aps);
            near(bot.aps_ci, ci);
            near(bot.pwin, pwin);
            near(bot.anpp, anpp);
            near(bot.vote, vote);
        }
        for (name, opponent, npp, knn_pbi) in PAIRINGS {
            let index = standings.by_name[name];
            let opponent = standings.by_name[opponent];
            let pair = standings.pairings[index].iter().find(|p| p.opponent == opponent).unwrap();
            near(pair.npp, npp);
            near(pair.knn_pbi, knn_pbi);
        }
    }

    #[test]
    fn nothing_to_score_gives_empty_standings() {
        let standings = Standings::compute(&[], &Tally::default());
        assert!(standings.bots.is_empty());
        assert!(!standings.stable);
    }
}
