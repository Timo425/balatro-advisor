//! How far an analysis has got, and stopping one that's no longer wanted (the live page's,
//! when the game writes a newer state). An analysis given a `Progress` (`advise::Options`)
//! counts its steps on it and, once `stop` is asked, unwinds at its next checkpoint (every
//! simulated decision, every item of its parallel work) with `Stopped`: catch it with
//! `std::panic::catch_unwind` and tell it from a crash by its payload (`is_stopped`).
//! The threads an analysis spreads its work over carry its `Progress` too (`current`, `enter`).
//!
//! A `Progress` can also carry a deadline (`with_deadline`): past it, each stage that can stop
//! does, with what it has (`time_up`: a race keeps its leader so far, `compare`; once a shop's
//! first ranking is held, the analysis unwinds with `TimeUp` and `advise::analyze` answers
//! with that ranking), saying so (`note`); the stages that can't stop yet run on and say how
//! far past it they ran. With no deadline nothing changes. The deadline can be lifted while the
//! analysis runs (`lift_deadline`: the live page's "let it finish").

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The steps `advise::analyze` counts (`step`): one per stage it reports with `BAV_TIMING`
pub const STEPS: usize = 19;

struct Inner {
    stop: AtomicBool,
    steps: AtomicUsize,
    /// When it started (the deadline counts from here)
    start: Instant,
    /// The deadline, in ms after `start` (`NONE`: none)
    deadline_ms: AtomicU64,
    /// A fallback answer is held (a shop's first ranking): past the deadline, checkpoints
    /// unwind with `TimeUp`
    fallback: AtomicBool,
    /// What stopped at the deadline, with what it kept (`note`)
    notes: Mutex<Vec<String>>,
    /// How many notes there were when the fallback was held (`hold_fallback`)
    notes_at_hold: AtomicUsize,
}

const NONE: u64 = u64::MAX;

impl Default for Inner {
    fn default() -> Inner {
        Inner { stop: AtomicBool::new(false), steps: AtomicUsize::new(0), start: Instant::now(), deadline_ms: AtomicU64::new(NONE), fallback: AtomicBool::new(false), notes: Mutex::new(vec![]), notes_at_hold: AtomicUsize::new(0) }
    }
}

/// One analysis' progress and its stop request (clones share them)
#[derive(Clone, Default)]
pub struct Progress(Arc<Inner>);

impl std::fmt::Debug for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Progress({}/{STEPS}{})", self.steps_done(), if self.stop_asked() { ", stop asked" } else { "" })
    }
}

impl Progress {
    pub fn new() -> Progress {
        Progress::default()
    }

    /// A progress whose analysis answers by `limit` from now (`None`: no deadline) with what it
    /// has (see the module's doc)
    pub fn with_deadline(limit: Option<Duration>) -> Progress {
        let p = Progress::default();
        if let Some(l) = limit {
            p.0.deadline_ms.store(l.as_millis().min(NONE as u128 - 1) as u64, Ordering::Relaxed);
        }
        p
    }

    /// The deadline, from the start (`None`: none, or lifted)
    pub fn deadline(&self) -> Option<Duration> {
        let ms = self.0.deadline_ms.load(Ordering::Relaxed);
        (ms != NONE).then(|| Duration::from_millis(ms))
    }

    /// No deadline from now on: the analysis runs to its end (stages already stopped stay so)
    pub fn lift_deadline(&self) {
        self.0.deadline_ms.store(NONE, Ordering::Relaxed);
    }

    /// Time since the start
    pub fn elapsed(&self) -> Duration {
        self.0.start.elapsed()
    }

    /// Whether a deadline is set and past
    pub fn past_deadline(&self) -> bool {
        self.deadline().is_some_and(|d| self.elapsed() >= d)
    }

    /// The analysis now holds a fallback answer: past the deadline it unwinds with `TimeUp` at
    /// its next checkpoint
    pub(crate) fn hold_fallback(&self) {
        let n = self.0.notes.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len();
        self.0.notes_at_hold.store(n, Ordering::Relaxed);
        self.0.fallback.store(true, Ordering::Relaxed);
    }

    /// The notes made before the fallback was held: what the fallback answer itself rests on
    pub(crate) fn notes_before_hold(&self) -> Vec<String> {
        let mut v = self.notes();
        v.truncate(self.0.notes_at_hold.load(Ordering::Relaxed));
        v
    }

    /// What stopped at the deadline, in the order it did (`note`)
    pub fn notes(&self) -> Vec<String> {
        self.0.notes.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// Ask the analysis to stop at its next checkpoint
    pub fn stop(&self) {
        self.0.stop.store(true, Ordering::Relaxed);
    }

    pub fn stop_asked(&self) -> bool {
        self.0.stop.load(Ordering::Relaxed)
    }

    /// Steps done (of `STEPS`)
    pub fn steps_done(&self) -> usize {
        self.0.steps.load(Ordering::Relaxed)
    }
}

/// What a stopped analysis unwinds with
pub struct Stopped;

/// Whether a caught panic's payload is a stop (`Stopped`) rather than a crash
pub fn is_stopped(payload: &(dyn std::any::Any + Send)) -> bool {
    payload.is::<Stopped>()
}

/// What an analysis past its deadline unwinds with once it holds a fallback answer
/// (`hold_fallback`); `advise::analyze` catches it and answers with that
pub struct TimeUp;

/// Whether a caught panic's payload is `TimeUp`
pub fn is_time_up(payload: &(dyn std::any::Any + Send)) -> bool {
    payload.is::<TimeUp>()
}

thread_local! {
    static CURRENT: RefCell<Option<Progress>> = const { RefCell::new(None) };
}

/// The progress of the analysis this thread works for, if any
pub(crate) fn current() -> Option<Progress> {
    CURRENT.with(|c| c.borrow().clone())
}

/// This thread works for the analysis `p` until the guard drops (then for whatever it did
/// before)
pub(crate) fn enter(p: Option<Progress>) -> impl Drop {
    struct Guard(Option<Progress>);
    impl Drop for Guard {
        fn drop(&mut self) {
            let prev = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = prev);
        }
    }
    Guard(CURRENT.with(|c| std::mem::replace(&mut *c.borrow_mut(), p)))
}

/// One step of the analysis done
pub(crate) fn step() {
    CURRENT.with(|c| {
        if let Some(p) = c.borrow().as_ref() {
            p.0.steps.fetch_add(1, Ordering::Relaxed);
        }
    });
}

/// Unwind with `Stopped` if this thread's analysis was asked to stop, with `TimeUp` if it's past
/// its deadline holding a fallback answer (no panic message: neither is a crash)
pub(crate) fn checkpoint() {
    let (stop, time_up) = CURRENT.with(|c| c.borrow().as_ref().map_or((false, false), |p| (p.stop_asked(), p.0.fallback.load(Ordering::Relaxed) && p.past_deadline())));
    if stop {
        std::panic::resume_unwind(Box::new(Stopped));
    }
    if time_up {
        std::panic::resume_unwind(Box::new(TimeUp));
    }
}

/// Whether this thread's analysis has a deadline (a race then samples in pieces it can stop
/// between: `compare`)
pub(crate) fn has_deadline() -> bool {
    CURRENT.with(|c| c.borrow().as_ref().is_some_and(|p| p.deadline().is_some()))
}

/// Whether this thread's analysis is past its deadline: a stage that can stop does
pub(crate) fn time_up() -> bool {
    CURRENT.with(|c| c.borrow().as_ref().is_some_and(Progress::past_deadline))
}

/// Say what stopped at the deadline and what it kept (for the analysis' `time_limit`)
pub(crate) fn note(what: String) {
    CURRENT.with(|c| {
        if let Some(p) = c.borrow().as_ref() {
            p.0.notes.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(what);
        }
    });
}


/// Every worker joined, their results in order; where some panicked, a stop (`Stopped`,
/// `TimeUp`) is passed on before any other panic: a worker that unwound with one while holding
/// a lock leaves it poisoned, and another worker's panic on that lock is its echo, not a crash
pub(crate) fn joined_all<'s, R>(handles: impl IntoIterator<Item = std::thread::ScopedJoinHandle<'s, R>>) -> Vec<R> {
    let results: Vec<std::thread::Result<R>> = handles.into_iter().map(|h| h.join()).collect();
    if results.iter().any(|r| r.is_err()) {
        let mut errors: Vec<Box<dyn std::any::Any + Send>> = results.into_iter().filter_map(|r| r.err()).collect();
        let at = errors.iter().position(|e| is_stopped(&**e) || is_time_up(&**e)).unwrap_or(0);
        std::panic::resume_unwind(errors.swap_remove(at));
    }
    results.into_iter().map(|r| r.unwrap_or_else(|_| unreachable!())).collect()
}

/// Runs `f` over `items` on all cores, keeping order: the analysis' fan-out (`advise`) and a big
/// hand's screen (`sim::big_hand_moves`). Each worker takes the next item not yet taken, so items
/// that cost more than others don't leave cores idle. The workers work for this thread's
/// analysis: a stop reaches them (`checkpoint`), and a worker's panic passes on as it was
/// (`joined_all`).
pub(crate) fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(items.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    // the workers work for this thread's analysis (`progress`): a stop reaches them
    let p = current();
    let mut done: Vec<(usize, R)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let (p, f, next) = (p.clone(), &f, &next);
                s.spawn(move || {
                    let _p = enter(p);
                    let mut out = vec![];
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(x) = items.get(i) else { break };
                        checkpoint();
                        out.push((i, f(x)));
                    }
                    out
                })
            })
            .collect();
        joined_all(handles).into_iter().flatten().collect()
    });
    done.sort_unstable_by_key(|x| x.0);
    done.into_iter().map(|x| x.1).collect()
}

/// `par_map`, where a worker takes no more items once `stop()` says so (from item `min` on: the
/// items before it are always done). The items done are those before the first `None`; an
/// item after a hole may be done too (`stop` can turn false again: a lifted deadline).
pub(crate) fn par_map_until<T: Sync, R: Send>(items: &[T], min: usize, stop: impl Fn() -> bool + Sync, f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    let idx: Vec<usize> = (0..items.len()).collect();
    let halted = AtomicBool::new(false);
    par_map(&idx, |&i| {
        if i >= min && (halted.load(Ordering::Relaxed) || stop()) {
            halted.store(true, Ordering::Relaxed);
            return None;
        }
        Some(f(&items[i]))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn par_map_keeps_the_items_order_whatever_each_costs() {
        // the workers take items one at a time, the costly ones (every seventh) taking longer
        let items: Vec<u64> = (0..300).collect();
        let out = par_map(&items, |&x| {
            let mut s = x;
            for _ in 0..if x % 7 == 0 { 200_000 } else { 10 } {
                s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            }
            (x, s)
        });
        assert!(out.iter().enumerate().all(|(i, o)| o.0 == i as u64));
    }

    #[test]
    fn a_stop_reaches_the_threads_and_unwinds_as_stopped() {
        let p = Progress::new();
        let _g = enter(Some(p.clone()));
        step();
        assert_eq!(p.steps_done(), 1);
        checkpoint();
        p.stop();
        let inherited = current();
        let r = std::thread::scope(|s| {
            let h = s.spawn(move || {
                let _g = enter(inherited);
                checkpoint();
            });
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| joined_all([h])))
        });
        assert!(r.is_err_and(|e| is_stopped(&*e)));
        // a thread working for no analysis never stops
        std::thread::spawn(checkpoint).join().unwrap();
    }

    #[test]
    fn a_deadline_is_past_once_its_time_is_up_and_unwinds_only_with_a_fallback_held() {
        let p = Progress::with_deadline(Some(Duration::ZERO));
        let _g = enter(Some(p.clone()));
        assert!(has_deadline() && time_up());
        // no fallback held: the checkpoint lets the analysis run on (the stages that can stop do)
        checkpoint();
        p.hold_fallback();
        let r = std::panic::catch_unwind(checkpoint);
        assert!(r.is_err_and(|e| is_time_up(&*e) && !is_stopped(&*e)));
        // lifted: no deadline, nothing past it
        p.lift_deadline();
        assert!(!has_deadline() && !time_up());
        checkpoint();
        // no deadline set at all: never past it
        let q = Progress::with_deadline(None);
        assert!(q.deadline().is_none() && !q.past_deadline());
    }

    #[test]
    fn par_map_until_does_the_items_before_min_and_stops_taking_after() {
        let items: Vec<usize> = (0..100).collect();
        let out = par_map_until(&items, 10, || true, |&x| x);
        assert!(out[..10].iter().enumerate().all(|(i, o)| *o == Some(i)));
        assert!(out[10..].iter().all(Option::is_none));
        let all = par_map_until(&items, 0, || false, |&x| x);
        assert!(all.iter().enumerate().all(|(i, o)| *o == Some(i)));
    }
}
