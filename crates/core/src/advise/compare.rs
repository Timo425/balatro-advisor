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
pub const EQUAL: f64 = 0.01;
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
/// more for a round you usually lose (a small leader's value). With every round seen worth
/// nothing (a round you rarely win, none won yet) nothing is shown equal: the round that would
/// part them could be worth anything.
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
    (better_by(a, b, lost_outright), largest > 0.0 && 3.0 * largest <= k * EQUAL * scale && m.abs() + 2.0 * se < EQUAL * scale)
}

/// How far `a` is shown better than `b` (`paired`'s test: the paired difference less 2
/// standard errors, with the round not yet seen counted in `b`'s favour where a round can be
/// lost outright): above 0 is clearly better
pub(super) fn better_by(a: &[f64], b: &[f64], lost_outright: bool) -> f64 {
    let mut d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    if lost_outright {
        d.push(-a.iter().chain(b).fold(0.0f64, |x, y| x.max(y.abs())));
    }
    let (m, se) = mean_se(&d);
    m - 2.0 * se
}

/// What an estimate's own samples say of its error, where it isn't a mean of rounds but worked
/// out from several (the shop ranking's measure: `advise` `rank_options`): for each set of
/// simulated rounds it rests on (`parts`: which set, then each round's influence on the
/// estimate as a share of it: its slope on that set's mean × the round's distance from the
/// mean ÷ the rounds, over the estimate), so the estimate's variance is the sum of the squares
/// (× the estimate squared). Round i of a set is the same draw for
/// every option, so two options' influences pair up round by round (`apart`): what both owe to
/// the same lucky round cancels, as in `paired`.
#[derive(Debug, Clone, Default)]
pub struct Noise {
    /// (the set, each round's influence: read with the estimate's slope were its luck good,
    /// the truth below it, and were it bad)
    pub parts: Vec<(u8, Vec<[f64; 2]>)>,
}

impl Noise {
    /// `rounds`' influence on an estimate whose slope on their mean is `slope` (below the
    /// mean, above it: the same where the estimate is a straight line in it): one more part
    /// (added to the part of the same set, round by round, when there is one)
    pub(super) fn add(&mut self, set: u8, slope: (f64, f64), rounds: impl ExactSizeIterator<Item = f64>) {
        let n = rounds.len().max(1) as f64;
        let x: Vec<f64> = rounds.collect();
        let mean = x.iter().sum::<f64>() / n;
        let at = match self.parts.iter().position(|p| p.0 == set) {
            Some(at) => at,
            None => {
                self.parts.push((set, vec![]));
                self.parts.len() - 1
            }
        };
        let part = &mut self.parts[at].1;
        if part.len() < x.len() {
            part.resize(x.len(), [0.0; 2]);
        }
        for (p, v) in part.iter_mut().zip(&x) {
            p[0] += slope.0 * (v - mean) / n;
            p[1] += slope.1 * (v - mean) / n;
        }
    }
}

/// How far luck alone may have put estimate `a` (its value `at`) above `b` (`bt`): the standard
/// error of their difference from their rounds' influences (each a share of its estimate),
/// paired set by set and round by round (a set or a round only one rests on counts whole).
/// With slopes the same both ways this is the paired standard error of the difference. Where
/// an estimate bends, two readings, weighed by how far the two share their luck (the
/// correlation of their influences, none below 0):
/// - luck of their own: `a` was lucky and `b` unlucky, so `a` falls by its slope below and `b`
///   rises by its slope above;
/// - one luck (two options on one board's rounds): both were lucky, and `a` falls further than
///   `b` (counted where `a`'s fall is the larger), or both unlucky, and `b` rises further
///   (where `b`'s rise is the larger). A sure thing and a chance near none on the same rounds
///   aren't parted by the one's fall against the other's rise, which one luck can't do.
pub(super) fn over(a: &Noise, at: f64, b: &Noise, bt: f64) -> f64 {
    fn part(n: &Noise, set: u8) -> &[[f64; 2]] {
        n.parts.iter().find(|p| p.0 == set).map_or(&[], |p| p.1.as_slice())
    }
    let mut var = 0.0;
    let mut sets: Vec<u8> = a.parts.iter().chain(&b.parts).map(|p| p.0).collect();
    sets.sort_unstable();
    sets.dedup();
    for set in sets {
        let (x, y) = (part(a, set), part(b, set));
        // (a's fall against b's rise; both falling; both rising; then for the share of luck
        // they have in common: the falls' product and each one's size, falling and rising)
        let (mut apart, mut lucky, mut unlucky) = (0.0, 0.0, 0.0);
        let (mut dot, mut fall_a, mut fall_b, mut rise_a, mut rise_b) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for i in 0..x.len().max(y.len()) {
            let (x, y) = (x.get(i).map_or([0.0; 2], |v| [v[0] * at, v[1] * at]), y.get(i).map_or([0.0; 2], |v| [v[0] * bt, v[1] * bt]));
            apart += (x[0] - y[1]) * (x[0] - y[1]);
            lucky += (x[0] - y[0]) * (x[0] - y[0]);
            unlucky += (y[1] - x[1]) * (y[1] - x[1]);
            dot += x[0] * y[0];
            fall_a += x[0] * x[0];
            fall_b += y[0] * y[0];
            rise_a += x[1] * x[1];
            rise_b += y[1] * y[1];
        }
        let shared = if fall_a > 0.0 && fall_b > 0.0 { (dot / (fall_a * fall_b).sqrt()).clamp(0.0, 1.0) } else { 0.0 };
        let one_luck = (if fall_a >= fall_b { lucky } else { 0.0 }).max(if rise_b >= rise_a { unlucky } else { 0.0 });
        var += (1.0 - shared) * apart + shared * one_luck;
    }
    var.sqrt()
}

/// A budget for `race` (`budget(rounds done)`): `table` ((up to this many rounds done, at most
/// this many options still undecided), its last row beyond), and with many options at least
/// half of the `n` raced for each doubling of the rounds after the `first` batch (successive
/// halving: `n` options, n/2 after the first batch, n/4 after twice its rounds, …, each stage
/// costing about half the first batch while the halving is above the table). A fixed cut on
/// the first batch alone drops good options where many share the same results on few rounds
/// (a round won or lost) or one trails by luck.
pub(super) fn halving(table: &'static [(usize, usize)], first: usize, n: usize) -> impl Fn(usize) -> usize {
    move |done: usize| {
        let fixed = table.iter().find(|b| done <= b.0).map_or(table[table.len() - 1].1, |b| b.1);
        fixed.max((n * first).div_ceil(2 * done.max(1)))
    }
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
    race_with(n, first, max, budget, sample, value, secondary, tie_order, lost_outright, None, 0)
}

/// `race`, where each round is sampled on its own (`sample_round(i, round)`: no round reads
/// another's), so a batch with fewer options still in than cores also splits each option's
/// rounds between them: the same race, the cores it leaves idle otherwise (Best play's last
/// batches keep a few moves for hundreds of rounds) put to work
#[allow(clippy::too_many_arguments)]
pub(super) fn race_by_round<T: Send + Sync>(
    n: usize,
    first: usize,
    max: usize,
    budget: impl Fn(usize) -> usize,
    sample_round: impl Fn(usize, usize) -> T + Sync,
    value: impl Fn(&T) -> f64,
    secondary: impl Fn(&T) -> f64,
    tie_order: impl Fn(usize, usize) -> std::cmp::Ordering,
    lost_outright: bool,
) -> Race<T> {
    let sample = |c: usize, rounds: std::ops::Range<usize>| rounds.map(|i| sample_round(c, i)).collect();
    race_with(n, first, max, budget, sample, value, secondary, tie_order, lost_outright, None, std::thread::available_parallelism().map_or(4, |c| c.get()))
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
    race_with(n, first, max, budget, sample, value, secondary, tie_order, lost_outright, keep, 0)
}

/// `race_keeping`; `cores`: with fewer options still in than these, each option's rounds split
/// between them, as `race_by_round` does (0: never)
#[allow(clippy::too_many_arguments)]
fn race_with<T: Send + Sync>(
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
    cores: usize,
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
        let new: Vec<Vec<T>> = if alive.len() < cores {
            // each option's rounds in pieces, about four for each core: put back together in order
            let pieces = (4 * cores).div_ceil(alive.len());
            let size = (to - done).div_ceil(pieces).max(1);
            let tasks: Vec<(usize, std::ops::Range<usize>)> = (0..alive.len()).flat_map(|k| (done..to).step_by(size).map(move |a| (k, a..(a + size).min(to)))).collect();
            let mut out: Vec<Vec<T>> = alive.iter().map(|_| vec![]).collect();
            for ((k, _), v) in tasks.iter().zip(par_map(&tasks, |(k, r)| sample(alive[*k], r.clone()))) {
                out[*k].extend(v);
            }
            out
        } else {
            par_map(&alive, |&c| sample(c, done..to))
        };
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
    fn estimates_on_the_same_rounds_are_apart_only_by_what_differs() {
        // an estimate with slope 2 on a chance from 100 rounds, half of them won: its own error
        // is 2 × 0.05; another on the same rounds with the same slope isn't apart from it at
        // all, one with slope 1 by the difference's error, one on other rounds (a set of its
        // own) by both errors together
        let won = |from: usize| (0..100).map(move |i| ((i + from) % 2) as f64);
        let noise = |set: u8, slope: (f64, f64), from: usize| {
            let mut n = Noise::default();
            n.add(set, slope, won(from));
            n
        };
        let a = noise(0, (2.0, 2.0), 0);
        assert!((over(&a, 1.0, &Noise::default(), 1.0) - 0.1).abs() < 1e-9);
        assert!(over(&a, 1.0, &noise(0, (2.0, 2.0), 0), 1.0) < 1e-12);
        assert!((over(&a, 1.0, &noise(0, (1.0, 1.0), 0), 1.0) - 0.05).abs() < 1e-9);
        assert!((over(&a, 1.0, &noise(1, (2.0, 2.0), 0), 1.0) - 0.1 * 2f64.sqrt()).abs() < 1e-9);
        // rounds that go the other way double it (one wins where the other loses)
        assert!((over(&a, 1.0, &noise(0, (2.0, 2.0), 1), 1.0) - 0.2).abs() < 1e-9);
        // two parts on one set add round by round, a shorter one's rounds the first of the set
        // (each of its 50 rounds weighs twice a round of 100: slope −1 there cancels the first
        // half of slope 2 on all 100)
        let mut both = noise(0, (2.0, 2.0), 0);
        both.add(0, (-1.0, -1.0), (0..50).map(|i| (i % 2) as f64));
        assert!((over(&both, 1.0, &Noise::default(), 1.0) - 0.1 * 0.5f64.sqrt()).abs() < 1e-9, "{}", over(&both, 1.0, &Noise::default(), 1.0));
        // an estimate that bends (slope 0.5 below its chance, 4 above: a chance near none):
        // luck can have put it above a sure thing by its slope below only, and the sure thing
        // above it by its slope above. Fails if one slope serves both ways
        let bent = noise(0, (0.5, 4.0), 0);
        assert!((over(&bent, 1.0, &Noise::default(), 1.0) - 0.025).abs() < 1e-9);
        assert!((over(&Noise::default(), 1.0, &bent, 1.0) - 0.2).abs() < 1e-9);
        // two on the same rounds bend the same way: one luck moved both, so they're apart by
        // how their slopes differ on one side of it (the larger), not by one's fall against
        // the other's rise
        assert!((over(&bent, 1.0, &noise(0, (0.4, 4.0), 0), 1.0) - 0.005).abs() < 1e-9);
        // and by the larger fall where only that differs (slope 2 against 1 below, 2 above for
        // both): not nothing, as one's fall against the other's rise would have it
        assert!((over(&a, 1.0, &noise(0, (1.0, 2.0), 0), 1.0) - 0.05).abs() < 1e-9);
        // each a share of its estimate: twice the estimate, twice the error
        assert!((over(&a, 2.0, &Noise::default(), 1.0) - 0.2).abs() < 1e-9);
    }

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
    fn options_that_win_rarely_arent_called_equal_before_a_round_is_won() {
        // A wins 1 round in 40, from round 20; B never wins. On the first 16 rounds both are
        // worth nothing every round: nothing there says how far apart they are, so they mustn't
        // be called equal (the race would end on it, B as good as A)
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> { r.map(|i| if o == 0 && i % 40 == 20 { 1.0 } else { 0.0 }).collect() };
        let race = race(2, 16, MAX, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), true);
        assert_eq!(race.leader, 0, "{:?}", race.left);
        assert!(!race.tied[1], "{:?}", race.left);
    }

    #[test]
    fn the_halving_budget_keeps_half_of_many_options_each_time_the_rounds_double() {
        // 436 moves from a first batch of 16: 218 after it, 109 after 32 rounds, 55 after 64;
        // never fewer than the table's, so a few options are cut by the table alone
        let table: &'static [(usize, usize)] = &[(32, 48), (64, 24), (128, 12), (256, 8)];
        let b = halving(table, 16, 436);
        assert_eq!((b(16), b(32), b(64), b(128), b(256), b(512), b(1024)), (218, 109, 55, 28, 14, 8, 8));
        let few = halving(table, 16, 60);
        assert_eq!((few(16), few(32), few(64), few(512)), (48, 48, 24, 8));
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
    fn a_race_by_round_is_the_race_with_each_options_rounds_split_between_the_cores() {
        // options whose every round is its own (a value of the option and the round alone):
        // `race_by_round` splits a batch's rounds between the cores once fewer options are left
        // than cores (here 8 cores asked for, and after the first batch room for 2), and finds
        // what `race` finds, sample for sample; on this machine's own cores too
        let noise = |o: usize, i: usize| ((i.wrapping_mul(2_654_435_761) ^ o.wrapping_mul(40_503)).wrapping_mul(2_246_822_519) % 1000) as f64 / 1000.0 - 0.5;
        let round = |o: usize, i: usize| 1.0 + 0.4 * noise(o, i) - 0.003 * o as f64;
        let sample = |o: usize, r: std::ops::Range<usize>| -> Vec<f64> { r.map(|i| round(o, i)).collect() };
        let a = race(6, 16, MAX, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), true);
        let b = race_with(6, 16, MAX, |_| 2, sample, |x| *x, |x| *x, |x, y| y.cmp(&x), true, None, 8);
        let c = race_by_round(6, 16, MAX, |_| 2, round, |x| *x, |x| *x, |x, y| y.cmp(&x), true);
        assert_eq!((&c.samples, c.leader, &c.left), (&a.samples, a.leader, &a.left));
        assert!(a.samples.iter().any(|v| v.len() > 16), "{:?}", a.left);
        assert_eq!((a.samples, a.leader, a.tied, a.undecided, a.left), (b.samples, b.leader, b.tied, b.undecided, b.left));
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
