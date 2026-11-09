//! Real game states the advice once got wrong, with what it must say now. They live in
//! tests/fixtures/private/ (git-ignored: real runs aren't committed); each file is
//! {"note", "state": `balatro-advisor state --json`, "expect": {...}}. Skipped when empty.
//! A fixture whose full expectation waits on a known gap keeps it in "pending_expect" (with
//! "pending_gap", the gap in docs/design.md): the test fails once all of it holds, so the fix
//! that closes the gap moves it back into "expect".

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
        // whether the best move uses card `c`: a label ("9♠"), or "#n", the nth card from the left
        // of your hand as it sits (a face-down card, or one of two alike: by its place, through
        // the move's `indices`)
        let uses = |c: &Value| match c.as_str().and_then(|s| s.strip_prefix('#')) {
            Some(n) => {
                let k = n.parse::<u64>().ok().and_then(|n| n.checked_sub(1)).unwrap_or_else(|| panic!("{name}: {c} is no place (\"#1\" is the leftmost card)"));
                bp["indices"].as_array().is_some_and(|v| v.iter().any(|i| i.as_u64() == Some(k)))
            }
            None => bp["cards"].as_array().is_some_and(|v| v.contains(c)),
        };
        let plays = bp["action"] == "play";
        // the expectations in `exp` that don't hold, each as "key = got, expected want"
        let missed = |exp: &Value| -> Vec<String> {
            let mut out = vec![];
            for (k, want) in exp.as_object().unwrap() {
                let got = match k.as_str() {
                    "best_action" => bp["action"].clone(),
                    "best_hand" => bp["hand"].clone(),
                    "best_use_first" => bp["use_first"].clone(),
                    // how many cards the best move plays or discards
                    "best_size" => Value::from(bp["cards"].as_array().map_or(0, Vec::len)),
                    "top_option" => a["options"][0]["label"].clone(),
                    // the hand that carries most of your points ("Hands that score your points")
                    "top_hand" => a["hand_mix"][0]["hand"].clone(),
                    // cards the best move must not use (play or discard)
                    "best_not_cards" => Value::Bool(want.as_array().unwrap().iter().all(|c| !uses(c))),
                    // cards the best move must not play (discarding them is fine)
                    "best_not_played" => Value::Bool(want.as_array().unwrap().iter().all(|c| !(plays && uses(c)))),
                    // cards the best move must play
                    "best_played" => Value::Bool(want.as_array().unwrap().iter().all(|c| plays && uses(c))),
                    // ["A", "B"]: option A ranks above option B
                    "above" => {
                        let pos = |l: &Value| a["options"].as_array().unwrap().iter().position(|o| &o["label"] == l);
                        let (x, y) = (pos(&want[0]), pos(&want[1]));
                        Value::Bool(x.is_some() && (y.is_none() || x < y))
                    }
                    // ["A", "B"]: option A's `rank` (the ranking's value) above option B's, whatever
                    // the bands of ties put first: for an expectation about how options are
                    // valued, which a band's edge moving with the top option mustn't decide
                    "rank_above" => {
                        let rank = |l: &Value| a["options"].as_array().unwrap().iter().find(|o| &o["label"] == l).and_then(|o| o["rank"].as_f64());
                        Value::Bool(rank(&want[0]).zip(rank(&want[1])).is_some_and(|(x, y)| x > y))
                    }
                    // {"Joker": x}: the joker's `score_share` (the typical best hand's drop
                    // without it, a fraction) is below x
                    "share_below" => {
                        let jokers = a["jokers"].as_array().unwrap();
                        Value::Bool(want.as_object().unwrap().iter().all(|(j, x)| jokers.iter().any(|r| r["name"] == j.as_str() && r["score_share"].as_f64().is_some_and(|s| s < x.as_f64().unwrap()))))
                    }
                    // {"j_key": "Joker name"}: the joker on offer (in the shop or an open pack,
                    // else among the shop pool's) replaces that joker (its pick, not a tie)
                    "sells" => {
                        let owned = a["jokers"].as_array().unwrap();
                        Value::Bool(want.as_object().unwrap().iter().all(|(key, sold)| {
                            let c = a["shop"].as_array().unwrap().iter().chain(a["pool"].as_array().unwrap()).find(|c| c["key"] == key.as_str());
                            c.and_then(|c| c["sell_index"].as_u64()).is_some_and(|i| owned.get(i as usize).is_some_and(|j| j["name"] == *sold))
                        }))
                    }
                    // {"j_key": "Joker name"}: the joker on offer is never shown replacing that
                    // joker (one whose sale frees no slot)
                    "sells_not" => {
                        let owned = a["jokers"].as_array().unwrap();
                        Value::Bool(want.as_object().unwrap().iter().all(|(key, sold)| {
                            let c = a["shop"].as_array().unwrap().iter().chain(a["pool"].as_array().unwrap()).find(|c| c["key"] == key.as_str());
                            c.is_some() && c.and_then(|c| c["sell_index"].as_u64()).is_none_or(|i| owned.get(i as usize).is_none_or(|j| j["name"] != *sold))
                        }))
                    }
                    other => panic!("{name}: unknown expectation {other}"),
                };
                let want = if matches!(k.as_str(), "above" | "rank_above" | "best_not_cards" | "best_not_played" | "best_played" | "share_below" | "sells" | "sells_not") { &Value::Bool(true) } else { want };
                if &got != want {
                    out.push(format!("{k} = {got}, expected {want}"));
                }
            }
            out
        };
        // a pending expectation that can never tell its gap is closed is a mistake, not a skip
        for k in v.as_object().unwrap().keys().filter(|k| k.starts_with("pending")) {
            assert!(k == "pending_expect" || k == "pending_gap", "{name}: unknown key {k}");
        }
        if !v["pending_expect"].is_null() {
            assert!(v["pending_expect"].as_object().is_some_and(|o| !o.is_empty()), "{name}: pending_expect must be a non-empty object");
        }
        let note = v["note"].as_str().unwrap_or("");
        failures.extend(missed(&v["expect"]).into_iter().map(|m| format!("{name}: {m} ({note})")));
        if v["pending_expect"].is_object() && missed(&v["pending_expect"]).is_empty() {
            failures.push(format!("{name}: its pending expectation holds now: move \"pending_expect\" back into \"expect\" and mark {} closed for it ({note})", v["pending_gap"].as_str().unwrap_or("its gap")));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// For refactoring: writes the full analysis of every fixture (timing fields dropped) to
/// `$BAV_SNAPSHOT_DIR`, so the output before and after a change can be compared with
/// `diff -r`. Run: `BAV_SNAPSHOT_DIR=/tmp/before cargo test --release --test replay -- --ignored snapshot`
/// (`BAV_ONLY=name` for some fixtures; `BAV_SEED` for another seed than 42: what noise alone changes)
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
    let only = std::env::var("BAV_ONLY").ok();
    let seed: u64 = std::env::var("BAV_SEED").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(42);
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !name.contains(o.as_str())) {
            continue;
        }
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let run: RunState = serde_json::from_value(v["state"].clone()).unwrap();
        let mut a = serde_json::to_value(advise::analyze(&run, GameData::bundled(), None, &advise::Options { sims: 300, seed, ..Default::default() })).unwrap();
        for k in ["elapsed_ms", "steps_ms", "save_age_secs", "live"] {
            a.as_object_mut().unwrap().remove(k);
        }
        std::fs::write(out.join(f.file_name().unwrap()), serde_json::to_string_pretty(&a).unwrap()).unwrap();
    }
}

/// Measures the search (`compare::race` in Best play) against a reference: every move it
/// considered played on `compare::MAX` rounds of its own, the best chosen there, and the pick
/// compared with it on another `compare::MAX` rounds (paired, with its standard error). For
/// each fixture in a blind and each seed in `BAV_SEEDS` (default 42,43,44): how far the pick
/// is below the best, its rank, and how many moves are within the 1% tie margin. A pick
/// passes when it could be within the tie margin: gap − 2·se ≤ 1%. Prints; asserts nothing.
/// Slow (`BAV_ONLY=name` for one fixture).
/// Run: `cargo test --release --test replay -- --ignored search_against_reference --nocapture`
#[test]
#[ignore]
fn search_against_reference() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let only = std::env::var("BAV_ONLY").ok();
    let seeds: Vec<u64> = std::env::var("BAV_SEEDS").unwrap_or_else(|_| "42,43,44".into()).split(',').filter_map(|s| s.trim().parse().ok()).collect();
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        if only.as_ref().is_some_and(|o| !name.contains(o.as_str())) {
            continue;
        }
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let run: RunState = serde_json::from_value(v["state"].clone()).unwrap();
        for &seed in &seeds {
            let a = advise::analyze(&run, GameData::bundled(), None, &advise::Options { sims: 300, seed, reference: true, ..Default::default() });
            let Some(bp) = a.best_play else { continue };
            let Some((gap, se)) = bp.reference_gap else { continue };
            let best = bp.reference[0].1;
            let pick = format!("{} {} [{}]", bp.action, bp.cards.join(" "), bp.use_first.clone().unwrap_or_default());
            let rank = bp.reference.iter().position(|(l, ..)| l == &pick);
            let within = bp.reference.iter().take_while(|(_, x, _)| *x >= best * 0.99).count();
            let verdict = if gap - 2.0 * se <= 0.01 { "ok" } else { "MISS" };
            println!("{verdict} {name} seed {seed}: pick {pick}: {:.2}% ± {:.2}% below the best, rank {:?} of {}, {within} within 1%", 100.0 * gap, 100.0 * se, rank.map(|r| r + 1), bp.reference.len());
            // `BAV_REF_TOP`: how many of the reference's moves to show (default 3)
            let top: usize = std::env::var("BAV_REF_TOP").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
            for (l, x, fate) in bp.reference.iter().take(top) {
                println!("    {x:.4} {l} ({fate})");
            }
        }
    }
}

/// Times the analysis of a built big face-down hand (gap 20, gap 25 (c)): the first House hand of
/// `house_all_face_down_burglar` (BAVANG10, Burglar: no discards) with the next six cards the
/// game would draw (the pile's last, cardarea.lua `CardArea:remove_card`), 14 cards in the rank
/// sort with the round's hand size raised to hold them, all face down, then the same cards face
/// up. Prints each pass's time, its steps and Best play's pick, and with `BAV_SNAPSHOT_DIR` writes
/// each analysis there as `snapshot` does (timings left out); asserts nothing. `BAV_FULL`: the
/// full pass, else the quick one the live page shows first; `BAV_WHICH=down` or `up`: one of them.
/// Run: `cargo test --release --test replay -- --ignored big_face_down_hand_timing --nocapture`
#[test]
#[ignore]
fn big_face_down_hand_timing() {
    let f = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private/house_all_face_down_burglar.json");
    let Ok(text) = std::fs::read_to_string(f) else { return };
    let v: Value = serde_json::from_str(&text).unwrap();
    let full = std::env::var_os("BAV_FULL").is_some();
    let which = std::env::var("BAV_WHICH").ok();
    for down in [true, false] {
        let name = if down { "down" } else { "up" };
        if which.as_ref().is_some_and(|w| w != name) {
            continue;
        }
        let mut s = v["state"].clone();
        let pile = s["draw_pile"].as_array().unwrap().clone();
        let n = pile.len();
        let mut hand = s["hand"].as_array().unwrap().clone();
        hand.extend(pile[n - 6..].iter().rev().cloned());
        // the game's rank sort: rank, then Spades, Hearts, Clubs, Diamonds (card.lua `Card:get_nominal`)
        let suit = |c: &Value| ["Diamonds", "Clubs", "Hearts", "Spades"].iter().position(|x| c["suit"] == *x).unwrap();
        hand.sort_by_key(|c| std::cmp::Reverse((c["rank"].as_i64().unwrap(), suit(c))));
        for c in hand.iter_mut() {
            c["face_down"] = Value::Bool(down);
        }
        s["hand"] = Value::Array(hand);
        s["draw_pile"] = Value::Array(pile[..n - 6].to_vec());
        s["round_hand_size_delta"] = 6.into();
        let run: RunState = serde_json::from_value(s).unwrap();
        let t = std::time::Instant::now();
        let a = advise::analyze(&run, GameData::bundled(), None, &advise::Options { quick: !full, ..Default::default() });
        let pick = a.best_play.as_ref().map(|b| format!("{} {} ({:.1}%)", b.action, b.cards.join(" "), 100.0 * b.p_win.unwrap_or(0.0))).unwrap_or_default();
        println!("{name}, {} pass: {:.1} s; Best play {pick}", if full { "full" } else { "quick" }, t.elapsed().as_secs_f64());
        println!("  {:?}", a.steps_ms);
        if let Some(dir) = std::env::var_os("BAV_SNAPSHOT_DIR") {
            let mut j = serde_json::to_value(&a).unwrap();
            for k in ["elapsed_ms", "steps_ms", "save_age_secs", "live"] {
                j.as_object_mut().unwrap().remove(k);
            }
            std::fs::create_dir_all(&dir).unwrap();
            let file = format!("big_face_down_hand_{name}_{}.json", if full { "full" } else { "quick" });
            std::fs::write(std::path::Path::new(&dir).join(file), serde_json::to_string_pretty(&j).unwrap()).unwrap();
        }
    }
}
