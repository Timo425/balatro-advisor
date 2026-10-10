//! The history check's judge (D12): for each saved state whose Best play pick moved between
//! two runs of `history_picks.rs` (before and after a change), the old pick against the new
//! one (`advise::Options::judge`: the oracle makes the policy's decisions that aren't a win on
//! the table; the valuation, the engine, the finish and consumable use are the code's own, so
//! a change to those is half judging itself: read the verdicts with that in mind). Slow;
//! prints. Run on the change:
//! `BAV_PICKS_BEFORE=/tmp/before BAV_PICKS_AFTER=/tmp/after cargo test --release --test
//! history -- --ignored history_judge --nocapture` (the same `BAV_SAMPLE`/`BAV_ONLY` as the
//! picks; `BAV_JUDGE_ROUNDS`, default 400). A pick moves when the cards' places in your hand
//! change (a new arrangement of the same cards isn't a move), or, with a consumable used first,
//! the cards it changes or adds. Worst first: BETTER / WORSE: clearly apart and by more than
//! the search's tie margin (`advise::TIE_MARGIN`); tie: within it both ways; unclear: neither;
//! ?: can't be judged (a consumable's old target can't be rebuilt, or the move isn't one now);
//! LOST: the change lost the pick. Every WORSE, unclear, ? and LOST is worth a look.

#[path = "history_picks.rs"]
mod history_picks;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use balatro_advisor::{advise, data::GameData, save};
use history_picks::{by_positions, name, states};

/// A pick: by positions in your hand ("action p,q,r [use first]", or "none", "load error"), and
/// as shown
type Pick = (String, String);

/// Each state's pick
fn picks(dir: &str) -> BTreeMap<String, Pick> {
    let text = std::fs::read_to_string(Path::new(dir).join("picks.tsv")).unwrap_or_else(|e| panic!("{dir}/picks.tsv: {e}"));
    text.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            Some((f.next()?.to_string(), (f.next()?.to_string(), f.next()?.to_string())))
        })
        .collect()
}

/// The cards a pick shows, in any order
fn cards_of(shown: &str) -> String {
    let mut v: Vec<&str> = shown.split_whitespace().collect();
    v.sort();
    v.join(" ")
}

/// For each state whose pick moved between `$BAV_PICKS_BEFORE` and `$BAV_PICKS_AFTER`: the
/// old pick against the new one, judged by the oracle (`advise::Options::judge`), worst first.
#[test]
#[ignore]
fn history_judge() {
    let (Ok(before), Ok(after)) = (std::env::var("BAV_PICKS_BEFORE"), std::env::var("BAV_PICKS_AFTER")) else { panic!("set BAV_PICKS_BEFORE and BAV_PICKS_AFTER") };
    let rounds: usize = std::env::var("BAV_JUDGE_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(400);
    let (before, after) = (picks(&before), picks(&after));
    let by_name: BTreeMap<String, PathBuf> = states().into_iter().map(|p| (name(&p), p)).collect();
    let consumable = |p: &Pick| !p.0.ends_with("[]") && p.0.contains(" [");
    let moved: Vec<(&String, &Pick, &Pick)> = after
        .iter()
        .filter_map(|(k, new)| before.get(k).filter(|old| old.0 != new.0 || (consumable(old) && cards_of(&old.1) != cards_of(&new.1))).map(|old| (k, old, new)))
        .collect();
    let only = |a: &BTreeMap<String, Pick>, b: &BTreeMap<String, Pick>| a.keys().filter(|k| !b.contains_key(*k)).cloned().collect::<Vec<_>>();
    let (only_before, only_after) = (only(&before, &after), only(&after, &before));
    println!("{} states in both, {} picks moved; only before: {} {:?}; only after: {} {:?}", after.keys().filter(|k| before.contains_key(*k)).count(), moved.len(), only_before.len(), only_before.iter().take(5).collect::<Vec<_>>(), only_after.len(), only_after.iter().take(5).collect::<Vec<_>>());
    let mut rows: Vec<(f64, String)> = vec![];
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for (k, old, new) in &moved {
        let line = format!("{k}: {} -> {}", old.1, new.1);
        let lost = matches!(new.0.as_str(), "none" | "load error");
        if lost || matches!(old.0.as_str(), "none" | "load error") || old.0 == new.0 {
            let (verdict, why) = if lost {
                ("LOST", "the change lost the pick")
            } else if old.0 == new.0 {
                ("?", "the same move with a consumable whose target or added cards changed: the old one can't be rebuilt")
            } else {
                ("new", "a pick where there was none")
            };
            *count.entry(verdict).or_default() += 1;
            rows.push((f64::NEG_INFINITY, format!("{verdict:7} {line}: {why}")));
            continue;
        }
        let today = if consumable(old) { " (its consumable's target as the code now chooses it)" } else { "" };
        let Some(p) = by_name.get(*k) else {
            *count.entry("state gone").or_default() += 1;
            rows.push((f64::NEG_INFINITY, format!("GONE    {line}: the state isn't in the history now")));
            continue;
        };
        let run = save::load(p, GameData::bundled()).unwrap();
        let opts = advise::Options { quick: true, judge: vec![old.0.clone()], judge_rounds: rounds, ..Default::default() };
        let bp = advise::analyze(&run, GameData::bundled(), None, &opts).best_play.unwrap();
        let note = if by_positions(&bp) == new.0 { String::new() } else { format!(" (this code picks {}: not the code \"after\" was made on)", by_positions(&bp)) };
        let Some((_, Some((d, se)))) = bp.judged.first().cloned() else {
            *count.entry("unjudged").or_default() += 1;
            rows.push((f64::NEG_INFINITY, format!("?       {line}: the old pick isn't a move that can be judged now{note}")));
            continue;
        };
        // the old pick's worth over the new one's, so the new pick's gain is its negative
        let (gain, tie) = (-d, advise::TIE_MARGIN);
        let verdict = if gain + 2.0 * se < 0.0 && gain < -tie {
            "WORSE"
        } else if gain - 2.0 * se > 0.0 && gain > tie {
            "BETTER"
        } else if gain - 2.0 * se >= -tie && gain + 2.0 * se <= tie {
            "tie"
        } else {
            "unclear"
        };
        *count.entry(verdict).or_default() += 1;
        rows.push((gain, format!("{verdict:7} {line}: {:+.2}% ± {:.2}%{note}{today}", 100.0 * gain, 100.0 * se)));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, r) in &rows {
        println!("{r}");
    }
    println!("moved {}: {}", moved.len(), count.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
}
