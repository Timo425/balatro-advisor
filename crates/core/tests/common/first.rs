//! A shop's first ranking (`advise::FirstSink`) against the finished analysis, for the tests
//! that hold both.
#![allow(dead_code)]

use balatro_advisor::advise::{Analysis, FirstSink, Options};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The first ranking an analysis hands over, and how long after `since` it came
pub type Caught = Arc<Mutex<Option<(u128, Analysis)>>>;

/// `opts` with a sink that keeps the first ranking
pub fn catching(opts: &Options, since: Instant) -> (Options, Caught) {
    let caught: Caught = Arc::default();
    let into = caught.clone();
    (Options { first: Some(FirstSink(Arc::new(move |a| *into.lock().unwrap() = Some((since.elapsed().as_millis(), a))))), ..opts.clone() }, caught)
}

/// Where the first ranking's options differ from the finished analysis': each must be one of
/// its options with the same numbers and note (the order, the ties and the rank's place are
/// among the options each holds)
pub fn differences(first: &Analysis, full: &Analysis) -> Vec<String> {
    let mut out = vec![];
    for o in &first.options {
        // (by the joker sold too: two copies of a joker share a label, each sold from its own slot)
        let Some(f) = full.options.iter().find(|f| f.label == o.label && f.kind == o.kind && f.sell_index == o.sell_index) else {
            out.push(format!("{}: not an option of the finished analysis", o.label));
            continue;
        };
        // (every field either has, but `tied`: a number the first ranking lacks is a difference too)
        let (a, b) = (serde_json::to_value(o).unwrap(), serde_json::to_value(f).unwrap());
        let (a, b) = (a.as_object().unwrap(), b.as_object().unwrap());
        let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).filter(|k| *k != "tied").collect();
        let show = |v: Option<&serde_json::Value>| v.map_or("none".to_string(), |x| x.to_string());
        for k in keys {
            if a.get(k) != b.get(k) {
                out.push(format!("{} {k}: first {}, finished {}", o.label, show(a.get(k)), show(b.get(k))));
            }
        }
    }
    for l in &first.pending {
        if !full.options.iter().any(|f| &f.label == l) {
            out.push(format!("{l}: named as still to come, not an option of the finished analysis"));
        }
    }
    for f in &full.options {
        if !first.options.iter().any(|o| o.label == f.label) && !first.pending.contains(&f.label) {
            out.push(format!("{}: an option of the finished analysis the first ranking neither holds nor names", f.label));
        }
    }
    out
}
