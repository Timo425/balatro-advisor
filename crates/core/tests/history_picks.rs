//! Best play's pick on the owner's saved states, for the history check (D12, `history.rs`
//! judges the picks a change moved). Uses only long-stable API, so it can be copied into a
//! `git worktree` of an older base commit and run there:
//! `BAV_PICKS=/tmp/before cargo test --release --test history_picks -- --ignored history_picks`.
//! Every state the live page saved in a blind (`~/.local/share/balatro-advisor/history/<seed>/
//! *.jkr`, never in the repo; `BAV_HISTORY` for another folder), the quick pass as the live page
//! shows first, 1-60 s a state by the Ante: one state in `BAV_SAMPLE` (default 3, about 120 of
//! 353 in 20 minutes; 1: all); `BAV_ONLY=text` for states whose path contains it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use balatro_advisor::{advise, calibration, data::GameData, save};

pub fn states() -> Vec<PathBuf> {
    let dir = std::env::var_os("BAV_HISTORY").map(PathBuf::from).or_else(|| calibration::default_dir().map(|d| d.join("history")));
    let Some(dir) = dir else { return vec![] };
    let only = std::env::var("BAV_ONLY").ok();
    // one state in `BAV_SAMPLE` (default 3; 1: all), by its name, so a state is in or out
    // whatever else the history holds (states saved between the two runs move nothing)
    let every: u64 = std::env::var("BAV_SAMPLE").ok().and_then(|s| s.parse().ok()).unwrap_or(3).max(1);
    let fnv = |s: &str| s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let mut out = vec![];
    for run in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(run.path()).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|x| x == "jkr") && only.as_ref().is_none_or(|o| p.to_string_lossy().contains(o.as_str())) && fnv(&name(&p)) % every == 0 {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub fn name(p: &Path) -> String {
    let run = p.parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    format!("{run}/{}", p.file_name().unwrap().to_string_lossy())
}

/// A pick by positions in your hand, sorted: the same move whatever order its cards are played
/// in ("action p,q,r [use first]")
pub fn by_positions(bp: &advise::PlayAdvice) -> String {
    let mut ix = bp.indices.clone();
    ix.sort();
    format!("{} {} [{}]", bp.action, ix.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","), bp.use_first.clone().unwrap_or_default())
}

/// Writes Best play's pick on every saved state in a blind to `$BAV_PICKS/picks.tsv`: the
/// state, the pick by positions, the pick as shown (a state that won't load or has no pick
/// gets a row saying so, so a change that loses picks shows).
#[test]
#[ignore]
fn history_picks() {
    let Some(out) = std::env::var_os("BAV_PICKS") else { panic!("set BAV_PICKS") };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let t0 = std::time::Instant::now();
    let mut lines = String::new();
    let mut n = 0;
    for p in states() {
        let run = match save::load(&p, GameData::bundled()) {
            Ok(r) => r,
            Err(e) => {
                lines += &format!("{}\tload error\t{e}\n", name(&p));
                continue;
            }
        };
        if !run.screen.in_blind() || run.hand.is_empty() {
            continue;
        }
        let t = std::time::Instant::now();
        let a = advise::analyze(&run, GameData::bundled(), None, &advise::Options { quick: true, ..Default::default() });
        eprintln!("{} {:.1?}", name(&p), t.elapsed());
        n += 1;
        lines += &match a.best_play {
            Some(bp) => format!("{}\t{}\t{} {} [{}]\n", name(&p), by_positions(&bp), bp.action, bp.cards.join(" "), bp.use_first.clone().unwrap_or_default()),
            None => format!("{}\tnone\tno Best play\n", name(&p)),
        };
    }
    std::fs::write(out.join("picks.tsv"), lines).unwrap();
    println!("{n} states ({:.0?})", t0.elapsed());
}
