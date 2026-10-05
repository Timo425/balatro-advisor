//! **Noise**: choosing between options whose values come from simulation. Every option is
//! sampled on the same draws (round i draws the same cards whatever the option), in batches
//! that double, each batch on fresh rounds: an option clearly worse than the leader (paired
//! difference, 95%; where a round can be lost outright, also past what a rare round not yet
//! seen could make up) stops, one provably as good (within `EQUAL` of the leader's value) is a
//! tie and stops, the rest go on, up to `MAX` rounds. Simulation goes where options are
//! close, and what can't be told apart is reported as a tie, never ranked by noise.
//!
//! When more options are still undecided than the budget allows (`budget`, by rounds done),
//! those least shown worse than the leader stay (the bound "clearly worse" uses: an option
//! whose rounds differ more from the leader's is less shown worse than a steady one just as
//! far behind), on exact ties by value, then a secondary value (e.g. points toward the target,
//! which still separates options in a round you almost always lose). That cut is a budget
//! limit, not a finding: those options aren't reported as worse or as ties.
//!
//! A tie is with the leader of its batch. If that leader is later found clearly worse, so is
//! what tied with it: a tie only stands when it leads, through ties, to the final leader.

use super::par_map;

/// The most rounds an option gets by default, and how close (as a share of the leader's value) counts
/// as equally good.
pub(super) const MAX: usize = 1600;
pub(super) const EQUAL: f64 = 0.01;
pub(super) struct Race<T> {
    /// Each option's samples (as many as it got)
    pub samples: Vec<Vec<T>>,
    pub leader: usize,
    /// Shown to be as good as the leader (within `EQUAL`)
    pub tied: Vec<bool>,
    /// Still in at the round cap: neither worse nor shown equal. Every caller reports these
    /// as ties (as good as far as these rounds can tell), never ranks them by noise
    pub undecided: Vec<bool>,
    /// How each option that stopped left: the rounds done then, and why ("worse", "equal" or
    /// "budget": cut to keep the race's cost, not a finding)
    pub left: Vec<Option<(usize, &'static str)>>,
}

/// Whether `a` is clearly better than `b` on the rounds both were sampled on (paired, 95%;
/// rounds of a graded measure, none lost outright: see `paired`)
pub(super) fn clearly_better(a: &[f64], b: &[f64]) -> bool {
    let k = a.len().min(b.len());
    k > 1 && paired(&a[..k], &b[..k], 1.0, false).0 > 0.0
}

/// The mean of `d` and its standard error
fn mean_se(d: &[f64]) -> (f64, f64) {
    let k = d.len() as f64;
    let m = d.iter().sum::<f64>() / k;
    (m, (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt())
}

/// How far `a` is shown better than `b` on the same draws (`better_by`: above 0 is clearly
/// better, paired difference, 95%), and whether they're provably within `EQUAL` (as a share of `scale`) of each other. Equal always
/// needs enough rounds (better too, where a round can be lost outright: below): a round where
/// the two part ways (one wins, the other loses) can be rare and worth up to the largest value
/// seen in a round, and when none has turned up in n rounds it may still happen up to 3 in n
/// (95%, the rule of three), so the rounds not yet seen could still move the difference by
/// 3 · largest / n. Equal needs that within
/// `EQUAL` · scale: about 300 rounds when a round is worth about the leader's value, many
/// more for a round you usually lose (a small leader's value).
///
/// `lost_outright`: a round can be lost outright (worth 0, as Best play's are), so the round
/// where they part ways is a jump worth up to the largest round value. Better then counts one
/// such round in `b`'s favour as if it had happened: with no spread in the rounds seen, that
/// asks for a difference past about 3 · largest / n (the rule of three), so a steady small edge
/// (say money) isn't a finding on a few rounds; once real parting rounds are in, their spread
/// is in the test and the extra round matters less. A graded measure (a deck's score ratio,
/// no such jump) is tested as it is.
fn paired(a: &[f64], b: &[f64], scale: f64, lost_outright: bool) -> (f64, bool) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let k = d.len() as f64;
    let (m, se) = mean_se(&d);
    let largest = a.iter().chain(b).fold(0.0f64, |x, y| x.max(y.abs()));
    (better_by(a, b, lost_outright), 3.0 * largest <= k * EQUAL * scale && m.abs() + 2.0 * se < EQUAL * scale)
}

/// How far `a` is shown better than `b` (`paired`'s test: the paired difference less 2
/// standard errors, with the round not yet seen counted in `b`'s favour where a round can be
/// lost outright): above 0 is clearly better
fn better_by(a: &[f64], b: &[f64], lost_outright: bool) -> f64 {
    let mut d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    if lost_outright {
        d.push(-a.iter().chain(b).fold(0.0f64, |x, y| x.max(y.abs())));
    }
    let (m, se) = mean_se(&d);
    m - 2.0 * se
}

/// `sample(i, range)`: option i's samples for rounds `range`; `value`: a sample's value;
/// `secondary`: what orders options the value can't separate; `first`: rounds in the first
/// batch; `max`: the most rounds an option gets (`MAX` unless the samples are costlier); `budget(rounds done)`: the most options still sampled after that many rounds;
/// `tie_order(x, y)`: how options with exactly equal means are ordered (`Greater` = x first),
/// so the result never depends on the order options are listed in; `lost_outright`: whether a
/// round can be lost outright (`paired`).
#[allow(clippy::too_many_arguments)]
pub(super) fn race<T: Send + Sync>(
    n: usize,
    first: usize,
    max: usize,
    budget: impl Fn(usize) -> usize,
    sample: impl Fn(usize, std::ops::Range<usize>) -> Vec<T> + Sync,
    value: impl Fn(&T) -> f64,
    secondary: impl Fn(&T) -> f64,
    tie_order: impl Fn(usize, usize) -> std::cmp::Ordering,
    lost_outright: bool,
) -> Race<T> {
    race_keeping(n, first, max, budget, sample, value, secondary, tie_order, lost_outright, None)
}

/// `race`, where option `keep` (the status quo: changing nothing) is never cut by the budget:
/// it leaves only on a finding (clearly worse, or shown equal), so "still in" or "as good"
/// says what the rounds showed about it, not what the budget dropped
#[allow(clippy::too_many_arguments)]
pub(super) fn race_keeping<T: Send + Sync>(
    n: usize,
    first: usize,
    max: usize,
    budget: impl Fn(usize) -> usize,
    sample: impl Fn(usize, std::ops::Range<usize>) -> Vec<T> + Sync,
    value: impl Fn(&T) -> f64,
    secondary: impl Fn(&T) -> f64,
    tie_order: impl Fn(usize, usize) -> std::cmp::Ordering,
    lost_outright: bool,
    keep: Option<usize>,
) -> Race<T> {
    let mut samples: Vec<Vec<T>> = (0..n).map(|_| Vec::new()).collect();
    let mut alive: Vec<usize> = (0..n).collect();
    let mut tied = vec![false; n];
    // whom each tied option was found as good as
    let mut anchor: Vec<Option<usize>> = vec![None; n];
    let mut left: Vec<Option<(usize, &'static str)>> = vec![None; n];
    let mean = |v: &[T], f: &dyn Fn(&T) -> f64| v.iter().map(f).sum::<f64>() / v.len().max(1) as f64;
    let (mut done, mut batch) = (0usize, first.max(1));
    let mut leader = 0usize;
    while done < max && !alive.is_empty() {
        let to = (done + batch).min(max);
        let new = par_map(&alive, |&c| sample(c, done..to));
        for (&c, v) in alive.iter().zip(new) {
            samples[c].extend(v);
        }
        done = to;
        batch = done;
        // best first: by value, then the secondary value, then a fixed order
        alive.sort_by(|&x, &y| {
            mean(&samples[y], &value).total_cmp(&mean(&samples[x], &value)).then(mean(&samples[y], &secondary).total_cmp(&mean(&samples[x], &secondary))).then(tie_order(y, x))
        });
        leader = alive[0];
        let lead: Vec<f64> = samples[leader].iter().map(&value).collect();
        let scale = mean(&samples[leader], &value).abs().max(1e-9);
        // how far the leader is shown better than each option (`better_by`)
        let mut shown = vec![f64::MIN; n];
        alive.retain(|&c| {
            if c == leader {
                return true;
            }
            let vals: Vec<f64> = samples[c].iter().map(&value).collect();
            let (by, equal) = paired(&lead, &vals, scale, lost_outright);
            shown[c] = by;
            if by > 0.0 {
                left[c] = Some((done, "worse"));
                return false;
            }
            if equal {
                tied[c] = true;
                anchor[c] = Some(leader);
                left[c] = Some((done, "equal"));
                return false;
            }
            true
        });
        // over the budget: keep the options least shown worse than the leader (`better_by`),
        // not the best by value so far, which on few rounds is a steady edge (say money)
        // before the rare round that separates them could show
        let room = budget(done).max(1);
        if alive.len() > room {
            alive.sort_by(|&x, &y| shown[x].total_cmp(&shown[y]));
        }
        let mut k = 0;
        alive.retain(|&c| {
            k += 1;
            let stays = k <= room || Some(c) == keep;
            if !stays {
                left[c] = Some((done, "budget"));
            }
            stays
        });
        if alive.len() == 1 {
            break;
        }
    }
    // still in at the cap: undecided (not worse, not shown equal)
    let mut undecided = vec![false; n];
    for &c in &alive {
        if c != leader {
            undecided[c] = true;
        }
    }
    // a tie with an option that didn't end as (or as good as) the leader doesn't stand
    let stands = |c: usize, anchor: &[Option<usize>]| {
        let mut x = c;
        for _ in 0..n {
            match anchor[x] {
                Some(a) if a == leader => return true,
                Some(a) => x = a,
                None => return false,
            }
        }
        false
    };
    let tied: Vec<bool> = (0..n).map(|c| c != leader && tied[c] && stands(c, &anchor)).collect();
    Race { samples, leader, tied, undecided, left }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_that_part_ways_rarely_arent_called_equal_on_a_few_rounds() {
        // B is A except for a lost round 1 in 40 (2.5% worse): identical on the first rounds,
        // where a tie would be called on no evidence about the rare round
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> { r.map(|i| if o == 1 && i % 40 == 39 { 0.0 } else { 1.0 }).collect() };
        let race = race(2, 16, MAX, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), true);
        assert_eq!(race.leader, 0);
        assert!(!race.tied[1] && !race.undecided[1]);
    }

    #[test]
    fn a_steady_small_edge_isnt_clearly_better_before_a_rare_round_could_show() {
        // A earns a little more every round (0.5%) but loses 1 round in 50, from round 49:
        // B is better by 1.5%. On the first 32 rounds A leads on every one, with no spread at
        // all; calling B clearly worse there would drop the better option before the round that
        // separates them could turn up. The same with B a hair ahead in one round, which adds
        // no real spread
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> {
            r.map(|i| if o == 1 { 0.995 } else if i % 50 == 49 { 0.0 } else if i == 7 { 0.994 } else { 1.0 }).collect()
        };
        let race = race(2, 32, MAX, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), true);
        assert_eq!(race.leader, 1, "{:?}", race.left);
    }

    #[test]
    fn the_budget_keeps_what_isnt_shown_worse_over_a_steady_small_edge() {
        // A and C earn steadily (C 0.1% less) but lose 1 round in 50, from round 49; B swings
        // (0.895 or 1.095, 0.995 on average) and never loses: the best, by 1.5%. After 32
        // rounds, with room for 2, B is last by value, but C's edge over it is a steady one
        // before C's rare lost round could show, while B's rounds differ from the leader's:
        // B is the one less shown worse, so it stays. Fails if the cut is by value
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> {
            r.map(|i| match o {
                1 => if i % 2 == 0 { 0.895 } else { 1.095 },
                _ if i % 50 == 49 => 0.0,
                0 => 1.0,
                _ => 0.999,
            })
            .collect()
        };
        let race = race(3, 32, MAX, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), true);
        assert_eq!(race.leader, 1, "{:?}", race.left);
    }

    #[test]
    fn the_status_quo_isnt_cut_by_the_budget() {
        // Five noisy options around the same value (changing nothing a little lower, not
        // clearly) and a budget of 2: the budget cuts on noise,
        // but option 0 (changing nothing) stays until a finding, so it ends in, not dropped.
        // Fails if `keep` is cut like the rest.
        let noise = |o: usize, i: usize| ((i.wrapping_mul(2_654_435_761) ^ o.wrapping_mul(40_503)).wrapping_mul(2_246_822_519) % 1000) as f64 / 1000.0 - 0.5;
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> { r.map(|i| 1.0 + 0.6 * noise(o, i) - if o == 0 { 0.02 } else { 0.0 }).collect() };
        let race = race_keeping(5, 16, 128, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), false, Some(0));
        assert_eq!(race.samples[0].len(), 128, "{:?}", race.samples.iter().map(|v| v.len()).collect::<Vec<_>>());
        assert!(race.leader == 0 || race.tied[0] || race.undecided[0]);
    }

    #[test]
    fn a_tie_with_an_early_leader_that_falls_behind_doesnt_stand() {
        // E leads for its first 600 rounds by luck and C ties with it there (equal on every
        // round, enough of them); F (noisy, so still in) takes the lead later and E turns out
        // clearly worse. C was only as good as E.
        let f = |i: usize| 1.0 + 0.6 * (i as f64 * 1.7).sin();
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> {
            r.map(|i| match o {
                0 | 1 => if i < 600 { 1.02 } else { 0.70 },  // E, C
                _ => f(i),                                   // F
            })
            .collect()
        };
        let race = race(3, 16, MAX, |_| 3, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), false);
        assert_eq!(race.leader, 2, "{:?}", race.samples.iter().map(|v| v.len()).collect::<Vec<_>>());
        assert!(!race.tied[0] && !race.tied[1] && !race.undecided[1], "{:?}", race.tied);
    }
}
