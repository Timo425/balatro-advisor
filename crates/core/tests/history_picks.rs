//! Best play's pick on the owner's saved states, for the history check (D12, `history.rs`
//! judges the picks a change moved). Uses only long-stable API, so it can be copied into a
//! `git worktree` of an older base commit and run there:
//! `BAV_PICKS=/tmp/before cargo test --release --test history_picks -- --ignored history_picks`.
//! A fixed-size sample of the states the live page saved (`~/.local/share/balatro-advisor/
//! history/<seed>/*.jkr`, never in the repo; `BAV_HISTORY` for another folder), whatever the
//! history's size: `BAV_SAMPLE` states (default 350, over a third of them in a blind; 0: all,
//! today's too), the quick pass as the live page shows first on those in a blind, 1-60 s a
//! state by the Ante, about 15 minutes a run (136 in a blind in 13 min on 2026-10-17);
//! `BAV_ONLY=text` for the sampled states whose path contains it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use balatro_advisor::{advise, calibration, data::GameData, save};

pub fn states() -> Vec<PathBuf> {
    sample(std::env::var("BAV_SAMPLE").ok().and_then(|s| s.parse().ok()).unwrap_or(350))
}

/// `size` of the saved states, chosen by name (0: all, today's too); `BAV_HISTORY`, `BAV_ONLY`
/// as for `states`
pub fn sample(size: usize) -> Vec<PathBuf> {
    let dir = std::env::var_os("BAV_HISTORY").map(PathBuf::from).or_else(|| calibration::default_dir().map(|d| d.join("history")));
    let Some(dir) = dir else { return vec![] };
    let only = std::env::var("BAV_ONLY").ok();
    let mut out = vec![];
    for run in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(run.path()).into_iter().flatten().flatten() {
            let p = f.path();
            if p.extension().is_some_and(|x| x == "jkr") {
                out.push(p);
            }
        }
    }
    if size > 0 {
        // the `size` states lowest by the hash of their name (FNV-1a) among those saved before
        // today (UTC, by the time the live page names them with, crates/cli/src/ui.rs
        // `keep_history`; a name without one counts as old): the cost stays the same however
        // much is played, and the before and after runs of a check on the same day pick the
        // same states, since states saved in between aren't candidates yet (a fixed hash
        // threshold would keep a fixed share, growing with the history; the lowest hashes of
        // all states would let a new state push one out between the runs)
        const DAY_MS: u128 = 86_400_000;
        let today = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) / DAY_MS * DAY_MS;
        let saved = |p: &Path| p.file_name().and_then(|n| n.to_string_lossy().split('-').next()?.parse::<u128>().ok());
        let fnv = |s: &str| s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        out.retain(|p| saved(p).is_none_or(|ms| ms < today));
        out.sort_by_cached_key(|p| (fnv(&name(p)), name(p)));
        out.truncate(size);
    }
    out.retain(|p| only.as_ref().is_none_or(|o| p.to_string_lossy().contains(o.as_str())));
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
