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

use super::par_map;

/// The most rounds an option gets by default, and how close (as a share of the leader's value) counts
/// as equally good.
pub(super) const MAX: usize = 1600;
pub(super) const EQUAL: f64 = 0.01;

pub(super) struct Race<T> {
    /// Each option's samples (as many as it got)
    pub samples: Vec<Vec<T>>,
    pub leader: usize,
    /// As good as the leader, as far as can be told
    pub tied: Vec<bool>,
}

/// Whether `a` is clearly better than `b` on the same draws (paired difference, 95%), and
/// whether they're provably within `EQUAL` (as a share of `scale`) of each other.
fn paired(a: &[f64], b: &[f64], scale: f64) -> (bool, bool) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let k = d.len() as f64;
    let m = d.iter().sum::<f64>() / k;
    let se = (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt();
    (m - 2.0 * se > 0.0, m.abs() + 2.0 * se < EQUAL * scale)
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
                return false;
            }
            true
        });
        alive.truncate(budget(done).max(1));
        if alive.len() == 1 {
            break;
        }
    }
    // still undecided at the cap: as good as the leader as far as can be told
    for &c in &alive {
        if c != leader {
            tied[c] = true;
        }
    }
    Race { samples, leader, tied }
}
