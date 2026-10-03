//! **Noise**: choosing between options whose values come from simulation. Every option is
//! sampled on the same draws (round i draws the same cards whatever the option), in batches
//! that double, each batch on fresh rounds: an option clearly worse than the leader (paired
//! difference, 95%) stops, one provably as good (within `EQUAL` of the leader's value) is a tie
//! and stops, the rest go on, up to `MAX` rounds. Simulation goes where options are close, and
//! what can't be told apart is reported as a tie, never ranked by noise.
//!
//! When more options are still undecided than the budget allows (`budget`, by rounds done),
//! the rest are dropped by their value so far, then by a secondary value (e.g. points toward
//! the target, which still separates options in a round you almost always lose). That cut is
//! a budget limit, not a finding: those options aren't reported as worse or as ties.
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
}

/// Whether `a` is clearly better than `b` on the rounds both were sampled on (paired, 95%)
pub(super) fn clearly_better(a: &[f64], b: &[f64]) -> bool {
    let k = a.len().min(b.len());
    k > 1 && paired(&a[..k], &b[..k], 1.0).0
}

/// Whether `a` is clearly better than `b` on the same draws (paired difference, 95%), and
/// whether they're provably within `EQUAL` (as a share of `scale`) of each other. Equal also
/// needs enough rounds: a round where the two part ways (one wins, the other loses) can be
/// rare and worth up to the largest value seen in a round, and when none has turned up in
/// n rounds it may still happen up to 3 in n (95%, the rule of three). So equal needs
/// 3 · largest / n ≤ `EQUAL` · scale: about 300 rounds when a round is worth about the
/// leader's value, many more for a round you usually lose (a small leader's value).
fn paired(a: &[f64], b: &[f64], scale: f64) -> (bool, bool) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let k = d.len() as f64;
    let m = d.iter().sum::<f64>() / k;
    let se = (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt();
    let largest = a.iter().chain(b).fold(0.0f64, |x, y| x.max(y.abs()));
    (m - 2.0 * se > 0.0, 3.0 * largest <= k * EQUAL * scale && m.abs() + 2.0 * se < EQUAL * scale)
}

/// `sample(i, range)`: option i's samples for rounds `range`; `value`: a sample's value;
/// `secondary`: what orders options the value can't separate; `first`: rounds in the first
/// batch; `max`: the most rounds an option gets (`MAX` unless the samples are costlier); `budget(rounds done)`: the most options still sampled after that many rounds;
/// `tie_order(x, y)`: how options with exactly equal means are ordered (`Greater` = x first),
/// so the result never depends on the order options are listed in.
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
) -> Race<T> {
    let mut samples: Vec<Vec<T>> = (0..n).map(|_| Vec::new()).collect();
    let mut alive: Vec<usize> = (0..n).collect();
    let mut tied = vec![false; n];
    // whom each tied option was found as good as
    let mut anchor: Vec<Option<usize>> = vec![None; n];
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
        alive.retain(|&c| {
            if c == leader {
                return true;
            }
            let vals: Vec<f64> = samples[c].iter().map(&value).collect();
            let (worse, equal) = paired(&lead, &vals, scale);
            if worse {
                return false;
            }
            if equal {
                tied[c] = true;
                anchor[c] = Some(leader);
                return false;
            }
            true
        });
        alive.truncate(budget(done).max(1));
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
    Race { samples, leader, tied, undecided }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_that_part_ways_rarely_arent_called_equal_on_a_few_rounds() {
        // B is A except for a lost round 1 in 40 (2.5% worse): identical on the first rounds,
        // where a tie would be called on no evidence about the rare round
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> { r.map(|i| if o == 1 && i % 40 == 39 { 0.0 } else { 1.0 }).collect() };
        let race = race(2, 16, MAX, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x));
        assert_eq!(race.leader, 0);
        assert!(!race.tied[1] && !race.undecided[1]);
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
        let race = race(3, 16, MAX, |_| 3, sample, |x| *x, |x| *x, |x, y| y.cmp(&x));
        assert_eq!(race.leader, 2, "{:?}", race.samples.iter().map(|v| v.len()).collect::<Vec<_>>());
        assert!(!race.tied[0] && !race.tied[1] && !race.undecided[1], "{:?}", race.tied);
    }
}
