//! **Noise**: choosing between options whose values come from simulation. Every option is
//! sampled on the same draws (round i draws the same cards whatever the option), in batches
//! that double: an option clearly worse than the leader (paired difference, 95%) stops, one
//! provably as good (within `EQUAL`) is a tie and stops, the rest go on, up to `MAX` rounds.
//! Simulation goes where options are close, and what can't be told apart is reported as a
//! tie, never ranked by noise.

use super::par_map;

/// Rounds in the first batch (each batch doubles), the most rounds an option gets, and how
/// close (as a share of the value) counts as equally good.
pub(super) const FIRST: usize = 64;
pub(super) const MAX: usize = 1600;
pub(super) const EQUAL: f64 = 0.01;

pub(super) struct Race<T> {
    /// Each option's samples (as many as it got)
    pub samples: Vec<Vec<T>>,
    pub leader: usize,
    /// As good as the leader, as far as can be told
    pub tied: Vec<bool>,
}

/// `sample(i, range)`: option i's samples for rounds `range`; `value`: a sample's value;
/// `tie_order(x, y)`: how options with exactly equal means are ordered (`Greater` = x first),
/// so the result never depends on the order options are listed in.
pub(super) fn race<T: Send + Sync>(
    n: usize,
    sample: impl Fn(usize, std::ops::Range<usize>) -> Vec<T> + Sync,
    value: impl Fn(&T) -> f64,
    tie_order: impl Fn(usize, usize) -> std::cmp::Ordering,
) -> Race<T> {
    let mut samples: Vec<Vec<T>> = (0..n).map(|_| Vec::new()).collect();
    let mut alive: Vec<usize> = (0..n).collect();
    let mut tied = vec![false; n];
    let mean = |v: &[T]| v.iter().map(&value).sum::<f64>() / v.len().max(1) as f64;
    let (mut done, mut batch) = (0usize, FIRST);
    let mut leader = 0usize;
    while done < MAX && !alive.is_empty() {
        let to = (done + batch).min(MAX);
        let new = par_map(&alive, |&c| sample(c, done..to));
        for (&c, v) in alive.iter().zip(new) {
            samples[c].extend(v);
        }
        done = to;
        batch = done;
        leader = *alive.iter().max_by(|&&x, &&y| mean(&samples[x]).total_cmp(&mean(&samples[y])).then(tie_order(x, y))).unwrap();
        let lead = &samples[leader];
        alive.retain(|&c| {
            if c == leader {
                return true;
            }
            // paired differences on the same draws
            let d: Vec<f64> = lead.iter().zip(&samples[c]).map(|(x, y)| value(x) - value(y)).collect();
            let k = d.len() as f64;
            let m = d.iter().sum::<f64>() / k;
            let se = (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt();
            if m - 2.0 * se > 0.0 {
                return false;
            }
            if m.abs() + 2.0 * se < EQUAL {
                tied[c] = true;
                return false;
            }
            true
        });
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
