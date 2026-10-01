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
                // ["A", "B"]: option A ranks above option B
                "above" => {
                    let pos = |l: &Value| a["options"].as_array().unwrap().iter().position(|o| &o["label"] == l);
                    let (x, y) = (pos(&want[0]), pos(&want[1]));
                    Value::Bool(x.is_some() && (y.is_none() || x < y))
                }
                other => panic!("{name}: unknown expectation {other}"),
            };
            let want = if k == "above" { &Value::Bool(true) } else { want };
            if &got != want {
                failures.push(format!("{name}: {k} = {got}, expected {want} ({})", v["note"].as_str().unwrap_or("")));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
