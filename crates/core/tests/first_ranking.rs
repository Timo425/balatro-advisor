//! A shop's first ranking (`advise::FirstSink`) on the saved history: when it comes, when the
//! full ranking does, what it costs, and whether its options have the finished analysis' numbers.
//!
//!     cargo test --release --test first_ranking -- --ignored first_ranking --nocapture
//!
//! The shop and pack states among the history check's sample (`history_picks::states`:
//! `BAV_SAMPLE`, `BAV_HISTORY`, `BAV_ONLY` as there), or the states `BAV_FIRST_LIST=<file>` lists
//! (a line each, under the history's folder). `BAV_FIRST_OUT=<folder>` writes `first.tsv` there,
//! one line a state: its file, the money, ms to the first ranking (empty: none made), ms to the
//! full ranking, ms in all, the options in each, the first ranking's top, the full ranking's top,
//! the place of the full top in the first ranking (empty: not in it), the differences, the
//! ms the first ranking itself took (the analysis' "first ranking" step: its options are valued
//! again for the full ranking), and the first ranking's top's value in the finished ranking as a
//! share of the finished top's (`ShopOption::rank`; 1: the same, or tied for value). A heavy job.

#[path = "common/first.rs"]
mod first;
#[path = "history_picks.rs"]
mod history_picks;

use balatro_advisor::{advise, calibration, data::GameData, save};
use history_picks::stamp;
use std::io::Write;
use std::path::PathBuf;

#[test]
#[ignore]
fn first_ranking_on_history() {
    let dir = std::env::var_os("BAV_HISTORY").map(PathBuf::from).or_else(|| calibration::default_dir().map(|d| d.join("history"))).unwrap_or_default();
    let files: Vec<PathBuf> = match std::env::var_os("BAV_FIRST_LIST") {
        Some(list) => std::fs::read_to_string(&list).unwrap_or_else(|e| panic!("{}: {e}", list.display())).lines().filter(|l| !l.trim().is_empty()).map(|l| dir.join(l.trim())).collect(),
        None => history_picks::states().into_iter().filter(|p| p.file_stem().is_some_and(|n| n.to_string_lossy().ends_with("-Shop") || n.to_string_lossy().ends_with("Pack"))).collect(),
    };
    let out = std::env::var_os("BAV_FIRST_OUT").map(PathBuf::from);
    let _run = stamp::start("first_ranking", out.as_deref());
    let mut tsv = out.as_ref().map(|o| {
        std::fs::create_dir_all(o).unwrap();
        stamp::set_aside(o, "first.tsv");
        std::fs::File::create(o.join("first.tsv")).unwrap()
    });
    let data = GameData::bundled();
    let (mut made, mut wrong) = (0, vec![]);
    for f in &files {
        let Ok(run) = save::load(f, data) else { continue };
        let started = std::time::Instant::now();
        let (opts, caught) = first::catching(&advise::Options::default(), started);
        let full = advise::analyze(&run, data, None, &opts);
        let early = caught.lock().unwrap().take();
        // the full ranking's time: the steps up to it
        let to_ranking: u128 = full.steps_ms.iter().scan(false, |done, s| (!std::mem::replace(done, s.0 == "ranking")).then_some(s.1)).sum();
        let name = f.strip_prefix(&dir).unwrap_or(f).display().to_string();
        let top = |a: &advise::Analysis| a.options.first().map_or(String::new(), |o| o.label.clone());
        let diffs = early.as_ref().map_or(vec![], |(_, e)| first::differences(e, &full));
        let place = early.as_ref().and_then(|(_, e)| e.options.iter().position(|o| o.label == top(&full)));
        let own = full.steps_ms.iter().find(|s| s.0 == "first ranking").map_or(0, |s| s.1);
        let value = |label: &str| full.options.iter().find(|o| o.label == label).and_then(|o| o.rank);
        let share = early.as_ref().and_then(|(_, e)| Some(value(&top(e))? / value(&top(&full))?.max(1e-12)));
        let line = format!(
            "{name}\t{}\t{}\t{to_ranking}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{own}\t{}",
            run.dollars,
            early.as_ref().map_or(String::new(), |e| e.0.to_string()),
            full.elapsed_ms,
            early.as_ref().map_or(0, |e| e.1.options.len()),
            full.options.len(),
            early.as_ref().map_or(String::new(), |e| top(&e.1)),
            top(&full),
            place.map_or(String::new(), |p| p.to_string()),
            diffs.join(" | "),
            share.map_or(String::new(), |x| format!("{x:.4}")),
        );
        println!("{line}");
        if let Some(t) = tsv.as_mut() {
            writeln!(t, "{line}").unwrap();
        }
        made += early.is_some() as usize;
        wrong.extend(diffs.into_iter().map(|d| format!("{name}: {d}")));
    }
    println!("{} states, a first ranking in {made}", files.len());
    assert!(files.is_empty() || made > 0, "no first ranking was made on any state");
    assert!(wrong.is_empty(), "a first ranking's option differs from the finished analysis':\n{}", wrong.join("\n"));
}
