//! How far an analysis has got, and stopping one that's no longer wanted (the live page's,
//! when the game writes a newer state). An analysis given a `Progress` (`advise::Options`)
//! counts its steps on it and, once `stop` is asked, unwinds at its next checkpoint (every
//! simulated decision, every item of its parallel work) with `Stopped`: catch it with
//! `std::panic::catch_unwind` and tell it from a crash by its payload (`is_stopped`).
//! The threads an analysis spreads its work over carry its `Progress` too (`current`, `enter`).

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// The steps `advise::analyze` counts (`step`): one per stage it reports with `BAV_TIMING`
pub const STEPS: usize = 19;

#[derive(Default)]
struct Inner {
    stop: AtomicBool,
    steps: AtomicUsize,
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

/// Unwind with `Stopped` if this thread's analysis was asked to stop (no panic message: it
/// isn't a crash)
pub(crate) fn checkpoint() {
    if CURRENT.with(|c| c.borrow().as_ref().is_some_and(Progress::stop_asked)) {
        std::panic::resume_unwind(Box::new(Stopped));
    }
}

/// A worker thread's result, its panic (a stop included) passed on as it was
pub(crate) fn joined<R>(r: std::thread::Result<R>) -> R {
    r.unwrap_or_else(|e| std::panic::resume_unwind(e))
}

/// Runs `f` over `items` on all cores, keeping order: the analysis' fan-out (`advise`) and a big
/// hand's screen (`sim::big_hand_moves`). Each worker takes the next item not yet taken, so items
/// that cost more than others don't leave cores idle. The workers work for this thread's
/// analysis: a stop reaches them (`checkpoint`), and a worker's panic passes on as it was
/// (`joined`).
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
        handles.into_iter().flat_map(|h| joined(h.join())).collect()
    });
    done.sort_unstable_by_key(|x| x.0);
    done.into_iter().map(|x| x.1).collect()
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
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| joined(h.join())))
        });
        assert!(r.is_err_and(|e| is_stopped(&*e)));
        // a thread working for no analysis never stops
        std::thread::spawn(checkpoint).join().unwrap();
    }
}
