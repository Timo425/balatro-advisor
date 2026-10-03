//! Real game states the advice once got wrong, with what it must say now. They live in
//! tests/fixtures/private/ (git-ignored: real runs aren't committed); each file is
//! {"note", "state": `balatro-advisor state --json`, "expect": {...}}. Skipped when empty.

use balatro_advisor::{advise, data::GameData, save::RunState};
use serde_json::Value;

#[test]
fn replayed_states_give_the_expected_advice() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let data = GameData::bundled();
    let opts = advise::Options { sims: 300, seed: 42, ..Default::default() };
    let mut failures = vec![];
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let run: RunState = serde_json::from_value(v["state"].clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let a = serde_json::to_value(advise::analyze(&run, data, None, &opts)).unwrap();
        let bp = &a["best_play"];
        for (k, want) in v["expect"].as_object().unwrap() {
            let got = match k.as_str() {
                "best_action" => bp["action"].clone(),
                "best_hand" => bp["hand"].clone(),
                "best_use_first" => bp["use_first"].clone(),
                "top_option" => a["options"][0]["label"].clone(),
                // the hand that carries most of your points ("Hands that score your points")
                "top_hand" => a["hand_mix"][0]["hand"].clone(),
                // cards the best move must not use (play or discard)
                "best_not_cards" => {
                    let cards = bp["cards"].as_array().cloned().unwrap_or_default();
                    Value::Bool(want.as_array().unwrap().iter().all(|c| !cards.contains(c)))
                }
                // ["A", "B"]: option A ranks above option B
                "above" => {
                    let pos = |l: &Value| a["options"].as_array().unwrap().iter().position(|o| &o["label"] == l);
                    let (x, y) = (pos(&want[0]), pos(&want[1]));
                    Value::Bool(x.is_some() && (y.is_none() || x < y))
                }
                other => panic!("{name}: unknown expectation {other}"),
            };
            let want = if k == "above" || k == "best_not_cards" { &Value::Bool(true) } else { want };
            if &got != want {
                failures.push(format!("{name}: {k} = {got}, expected {want} ({})", v["note"].as_str().unwrap_or("")));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// For refactoring: writes the full analysis of every fixture (timing fields dropped) to
/// `$BAV_SNAPSHOT_DIR`, so the output before and after a change can be compared with
/// `diff -r`. Run: `BAV_SNAPSHOT_DIR=/tmp/before cargo test --release --test replay -- --ignored snapshot`
#[test]
#[ignore]
fn snapshot() {
    let Some(out) = std::env::var_os("BAV_SNAPSHOT_DIR") else { panic!("set BAV_SNAPSHOT_DIR") };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for f in &files {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let run: RunState = serde_json::from_value(v["state"].clone()).unwrap();
        let mut a = serde_json::to_value(advise::analyze(&run, GameData::bundled(), None, &advise::Options { sims: 300, seed: 42, ..Default::default() })).unwrap();
        for k in ["elapsed_ms", "save_age_secs", "live"] {
            a.as_object_mut().unwrap().remove(k);
        }
        std::fs::write(out.join(f.file_name().unwrap()), serde_json::to_string_pretty(&a).unwrap()).unwrap();
    }
}

/// Measures the search (`compare::race` in Best play) against a reference: every move it
/// considered played on `compare::MAX` rounds of its own. For each fixture in a blind: the
/// pick's value as a share of the best reference value, its rank, and the moves within the
/// 1% tie margin of the best. Prints; asserts nothing. Slow.
/// Run: `cargo test --release --test replay -- --ignored search_against_reference --nocapture`
#[test]
#[ignore]
fn search_against_reference() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let only = std::env::var("BAV_ONLY").ok();
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !name.contains(o.as_str())) {
            continue;
        }
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let run: RunState = serde_json::from_value(v["state"].clone()).unwrap();
        let a = advise::analyze(&run, GameData::bundled(), None, &advise::Options { sims: 300, seed: 42, reference: true, ..Default::default() });
        let Some(bp) = a.best_play else { continue };
        if bp.reference.is_empty() {
            continue;
        }
        let best = bp.reference[0].1;
        let pick = format!("{} {} [{}]", bp.action, bp.cards.join(" "), bp.use_first.clone().unwrap_or_default());
        let rank = bp.reference.iter().position(|(l, _)| l == &pick);
        let val = rank.map(|r| bp.reference[r].1).unwrap_or(f64::NAN);
        let within = bp.reference.iter().take_while(|(_, x)| *x >= best * 0.99).count();
        println!("{name}: pick {pick} = {:.4} of best {:.4} ({:.2}%), rank {:?} of {}, {within} within 1% of the best", val, best, 100.0 * (1.0 - val / best), rank.map(|r| r + 1), bp.reference.len());
        for (l, x) in bp.reference.iter().take(5) {
            println!("    {x:.4} {l}");
        }
    }
}
