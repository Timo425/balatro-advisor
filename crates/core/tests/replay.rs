//! Real game states the advice once got wrong, with what it must say now. They live in
//! tests/fixtures/private/ (git-ignored: real runs aren't committed); each file is
//! {"note", "state": `balatro-advisor state --json`, "expect": {...}}. Skipped when empty.
//! A fixture whose full expectation waits on a known gap keeps it in "pending_expect" (with
//! "pending_gap", the gap in docs/design.md): the test fails once all of it holds, so the fix
//! that closes the gap moves it back into "expect".

#[path = "common/stamp.rs"]
mod stamp;
#[path = "common/first.rs"]
mod first;
#[path = "common/filter.rs"]
mod filter;

use balatro_advisor::{advise, data::GameData, save::RunState};
use serde_json::Value;

/// The fixtures a check runs on, sorted by name: (file, name, the fixture) for each that
/// `BAV_ONLY` (by name) and `BAV_HOLDS` (by its state) keep (`filter::keep`); none without the
/// fixtures' folder
fn fixtures() -> Vec<(std::path::PathBuf, String, Value)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private");
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    let mut files: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    let all = files
        .into_iter()
        .map(|f| {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
            (f.clone(), f.file_stem().unwrap().to_string_lossy().to_string(), v)
        })
        .collect();
    filter::keep(all, |(_, name, _)| name.clone(), |(_, _, v)| Some(v["state"].clone()))
}

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
        // (with a shop's first ranking asked for: each option in it must have the numbers the
        // finished analysis gives it, `advise::FirstSink`)
        let (with_first, caught) = first::catching(&opts, std::time::Instant::now());
        let analysis = advise::analyze(&run, data, None, &with_first);
        if let Some((_, early)) = caught.lock().unwrap().as_ref() {
            failures.extend(first::differences(early, &analysis).into_iter().map(|d| format!("{name}: first ranking: {d}")));
        }
        let a = serde_json::to_value(analysis).unwrap();
        // every target the advice names (a tarot's, a Best play "use X first"'s) is a place in
        // your hand labelled as the hand shows that card (`hand_labels`: debuffs marked, face-down
        // ones by place), one place a card, as a bot would select it. (Which deck card was valued
        // there is the unit test's: advise.rs `a_held_tarot_names_the_cards_it_was_valued_on_…`.)
        let names = balatro_advisor::model::hand_labels(&run.hand);
        let mut check_targets = |what: String, t: &Value| {
            let (places, cards) = (t["places"].as_array().unwrap(), t["cards"].as_array().unwrap());
            let at: Vec<Option<&String>> = places.iter().map(|p| names.get(p.as_u64().unwrap() as usize)).collect();
            if places.len() != cards.len() || at.iter().zip(cards).any(|(n, c)| n.is_none_or(|n| c.as_str() != Some(n.as_str()))) {
                failures.push(format!("{name}: {what}'s targets {t} aren't the hand's cards at those places ({names:?})"));
            }
        };
        for t in a["tarots"].as_array().into_iter().flatten().filter(|t| !t["targets"].is_null()) {
            check_targets(t["name"].as_str().unwrap_or("?").to_string(), &t["targets"]);
        }
        for o in std::iter::once(&a["best_play"]).chain(a["best_play"]["alternatives"].as_array().into_iter().flatten()).filter(|o| !o["use_targets"].is_null()) {
            check_targets(format!("use {}", o["use_first"]), &o["use_targets"]);
        }
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
                    // the blind on offer's skip-or-play verdict ("play", "skip", "close" or "not
                    // modelled"; null when no blind shows one)
                    "skip_verdict" => a["blinds"].as_array().and_then(|b| b.iter().find(|b| !b["skip"].is_null())).map_or(Value::Null, |b| b["skip"]["verdict"].clone()),
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

/// `BAV_HOLDS`'s terms on a built state (`common/filter.rs`)
#[test]
fn holds_terms() {
    use filter::{Term, parse};
    assert_eq!(parse("face_down, boss,hand:13,j_blackboard,"), vec![Term::FaceDown, Term::Boss, Term::Hand(13), Term::Key("j_blackboard".into())]);
    let s = serde_json::json!({
        "hand": [{"rank": 14, "suit": "Spades", "face_down": true}, {"rank": 2, "suit": "Clubs"}],
        "jokers": [{"key": "j_blackboard", "name": "Blackboard"}],
        "used_jokers": ["j_burglar"],
        "current_blind": {"key": "bl_house"},
        "shop": {"boosters": [{"key": "p_arcana_normal_3"}]},
        "tags": ["tag_double"],
    });
    let holds = |t: &str| parse(t)[0].holds(&s);
    assert!(holds("face_down") && holds("boss") && holds("hand:2") && !holds("hand:3"));
    assert!(holds("j_blackboard") && holds("p_arcana") && holds("tag_double") && holds("bl_house"));
    assert!(!holds("j_burglar"), "the run's history isn't what a state holds");
    let small = serde_json::json!({"hand": [{"rank": 2, "suit": "Clubs"}], "current_blind": {"key": "bl_small"}});
    assert!(!parse("boss")[0].holds(&small) && !parse("face_down")[0].holds(&small));
    let shop = serde_json::json!({"hand": [], "current_blind": null});
    assert!(!parse("boss")[0].holds(&shop));
    assert!(std::panic::catch_unwind(|| parse("facedown")).is_err());
    assert!(std::panic::catch_unwind(|| parse("hand:many")).is_err());
}

/// For refactoring: writes the full analysis of every fixture (timing fields dropped) to
/// `$BAV_SNAPSHOT_DIR`, so the output before and after a change can be compared with
/// `diff -r`. Run: `BAV_SNAPSHOT_DIR=/tmp/before cargo test --release --test replay -- --ignored snapshot`
/// (`BAV_ONLY=a,b` for the fixtures whose name contains any of these, `BAV_HOLDS` for those
/// holding what a change can reach, `common/filter.rs`; `BAV_SEED` for another seed than 42:
/// what noise alone changes; `BAV_SNAPSHOT_FIRST`, `BAV_SNAPSHOT_DEADLINE` below)
#[test]
#[ignore]
fn snapshot() {
    let Some(out) = std::env::var_os("BAV_SNAPSHOT_DIR") else { panic!("set BAV_SNAPSHOT_DIR") };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let _run = stamp::start("snapshot", Some(&out));
    let seed: u64 = std::env::var("BAV_SEED").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(42);
    for (f, _, v) in &fixtures() {
        let run: RunState = serde_json::from_value(v["state"].clone()).unwrap();
        // `BAV_SNAPSHOT_FIRST=1`: with a shop's first ranking asked for (discarded), so the
        // snapshot shows the finished analysis is the same either way (`advise::FirstSink`)
        let opts = advise::Options { sims: 300, seed, ..Default::default() };
        let opts = if std::env::var_os("BAV_SNAPSHOT_FIRST").is_some() { first::catching(&opts, std::time::Instant::now()).0 } else { opts };
        // `BAV_SNAPSHOT_DEADLINE=<s>`: with a deadline the analysis doesn't reach (an hour, say),
        // so the snapshot shows that one not reached changes nothing (D22: the races sample in
        // pieces they could stop between, a first ranking is held)
        let deadline: Option<f64> = std::env::var("BAV_SNAPSHOT_DEADLINE").ok().and_then(|s| s.trim().parse().ok());
        let opts = advise::Options { progress: deadline.map(|d| balatro_advisor::progress::Progress::with_deadline(Some(std::time::Duration::from_secs_f64(d)))), ..opts };
        let mut a = serde_json::to_value(advise::analyze(&run, GameData::bundled(), None, &opts)).unwrap();
        for k in ["elapsed_ms", "steps_ms", "save_age_secs", "live"] {
            a.as_object_mut().unwrap().remove(k);
        }
        std::fs::write(out.join(f.file_name().unwrap()), serde_json::to_string_pretty(&a).unwrap()).unwrap();
    }
}

/// An analysis past its deadline answers with what it has, labelled (D22): with a deadline of
/// none at all, a shop or an open pack that makes a first ranking answers with it (its
/// `pending` named), a blind select with the blinds' chances (skip-or-play and the boss reroll
/// named as not valued; the second given Director's Cut and $10, so the reroll is valued), its
/// answer before the outlook the full one but for it, and Best play in a blind with its leader
/// so far, saying so. On the first few fixtures of each kind (fewer simulations: a stop's
/// labels, not the advice, are what's tested)
#[test]
fn a_deadline_past_answers_with_what_it_has_said_so() {
    use balatro_advisor::progress::Progress;
    let (mut shops, mut blinds, mut selects) = (0, 0, 0);
    for (_, name, v) in &fixtures() {
        let mut run: RunState = serde_json::from_value(v["state"].clone()).unwrap();
        let in_shop = run.screen == balatro_advisor::save::Screen::Shop || run.screen.in_pack();
        let in_blind = run.screen.in_blind() && !run.hand.is_empty();
        let in_select = run.screen == balatro_advisor::save::Screen::BlindSelect;
        if (in_shop && shops >= 3) || (in_blind && blinds >= 3) || (in_select && selects >= 2) || !(in_shop || in_blind || in_select) {
            continue;
        }
        let reroll_given = in_select && selects == 1 && !run.vouchers.iter().any(|k| k == "v_directors_cut" || k == "v_retcon");
        if reroll_given {
            run.vouchers.push("v_directors_cut".into());
            run.dollars = run.dollars.max(10.0);
        }
        let opts = |progress| advise::Options { sims: 60, quick: in_blind, progress, ..Default::default() };
        let a = advise::analyze(&run, GameData::bundled(), None, &opts(Some(Progress::with_deadline(Some(std::time::Duration::ZERO)))));
        let Some(tl) = &a.time_limit else {
            // (nothing cut and done within a second of the deadline counts as in time,
            // `TimeLimit::of`: a fast machine on a small state; a cut race always says so)
            assert!(a.best_play.as_ref().is_none_or(|b| b.time_limit.is_none()), "{name}: past its deadline and not said");
            continue;
        };
        if in_select {
            // the blinds as the full analysis has them, their chances included, with nothing
            // valued that the full one values after them: its verdicts named as not valued
            // (with an hour's limit: the same answer as none, its last answer held caught)
            let (made, caught) = first::catching(&opts(Some(Progress::with_deadline(Some(std::time::Duration::from_secs(3600))))), std::time::Instant::now());
            let full = advise::analyze(&run, GameData::bundled(), None, &made);
            assert!(tl.held && !tl.first_ranking, "{name}: {tl:?}");
            assert_eq!(a.blinds.iter().map(|b| (&b.slot, b.p_win)).collect::<Vec<_>>(), full.blinds.iter().map(|b| (&b.slot, b.p_win)).collect::<Vec<_>>(), "{name}");
            assert!(a.blinds.iter().all(|b| b.skip.is_none() && b.reroll.is_none()), "{name}");
            assert_eq!(a.pending.contains(&"skip or play".to_string()), full.blinds.iter().any(|b| b.skip.is_some()), "{name}: {:?}", a.pending);
            assert_eq!(a.pending.contains(&"the boss reroll".to_string()), full.blinds.iter().any(|b| b.reroll.is_some()), "{name}: {:?}", a.pending);
            assert!(full.time_limit.is_none(), "{name}");
            // (the voucher given reaches a boss reroll, so the line above tests that half)
            assert!(!reroll_given || full.blinds.iter().any(|b| b.reroll.is_some()), "{name}: Director's Cut given, no boss reroll valued");
            // every part the full answer fills that the first one leaves empty is named
            let parts = [
                (a.jokers.is_empty() && !full.jokers.is_empty(), "your jokers' shares"),
                (a.hand_mix.is_empty() && !full.hand_mix.is_empty(), "the hands that score"),
                (a.tarots.is_empty() && !full.tarots.is_empty(), "the tarots' values"),
                (a.pool.is_empty() && a.rescue.is_empty() && !(full.pool.is_empty() && full.rescue.is_empty()), "the jokers the shop can offer"),
                (a.long_assumptions.is_empty() && !full.long_assumptions.is_empty(), "what By Ante 8 assumes"),
                (a.outlook.is_none() && full.outlook.is_some(), "the outlook"),
            ];
            for (left_out, part) in parts {
                assert!(!left_out || a.pending.contains(&part.to_string()), "{name}: {part} left out, not named: {:?}", a.pending);
            }
            // the answer held before the outlook: the full one but for the outlook, named
            let (_, before) = caught.lock().unwrap().take().expect("a blind select with a deadline holds its answer");
            assert_eq!(before.pending, vec!["the outlook".to_string()], "{name}");
            let fields = |a: &advise::Analysis| {
                let mut v = serde_json::to_value(a).unwrap();
                for k in ["outlook", "pending", "elapsed_ms", "steps_ms", "time_limit"] {
                    v.as_object_mut().unwrap().remove(k);
                }
                v
            };
            assert_eq!(fields(&before), fields(&full), "{name}");
            selects += 1;
        } else if in_shop {
            // a first ranking when one is made (none past the planet-level cap, nor in a pack none
            // of whose picks it holds): then it's the answer, the rest named
            let (made, caught) = first::catching(&opts(None), std::time::Instant::now());
            let full = advise::analyze(&run, GameData::bundled(), None, &made);
            if let Some((_, f)) = caught.lock().unwrap().take() {
                assert!(tl.first_ranking, "{name}: {tl:?}");
                assert_eq!(a.pending, f.pending, "{name}");
                assert_eq!(a.options.iter().map(|o| &o.label).collect::<Vec<_>>(), f.options.iter().map(|o| &o.label).collect::<Vec<_>>(), "{name}");
                assert!(full.time_limit.is_none(), "{name}");
                shops += 1;
            }
        } else if let Some(bp) = a.best_play.as_ref().filter(|b| b.time_limit.is_some()) {
            // (a race the first batch decides, one move left, isn't cut: nothing to say)
            assert!(tl.stopped.iter().any(|s| s.starts_with("Best play")), "{name}: {tl:?} {:?}", bp.time_limit);
            blinds += 1;
        }
    }
    if !fixtures().is_empty() {
        assert!(shops > 0 && blinds > 0 && selects > 0, "a first ranking answered on {shops} shops, Best play cut on {blinds} blinds, a blind select's answer so far on {selects}");
    }
}

/// Measures the search (`compare::race` in Best play) against a reference: every move it
/// considered played on `compare::MAX` rounds of its own, the best chosen there, and the pick
/// compared with it on another `compare::MAX` rounds (paired, with its standard error). For
/// each fixture in a blind and each seed in `BAV_SEEDS` (default 42,43,44): how far the pick
/// is below the best, its rank, and how many moves are within the 1% tie margin. A pick
/// passes when it could be within the tie margin: gap − 2·se ≤ 1%. Prints; asserts nothing.
/// Slow (`BAV_ONLY=a,b`, `BAV_HOLDS` as for `snapshot`, for some fixtures).
/// Run: `cargo test --release --test replay -- --ignored search_against_reference --nocapture`
#[test]
#[ignore]
fn search_against_reference() {
    let _run = stamp::start("search_against_reference", None);
    let seeds: Vec<u64> = std::env::var("BAV_SEEDS").unwrap_or_else(|_| "42,43,44".into()).split(',').filter_map(|s| s.trim().parse().ok()).collect();
    for (_, name, v) in &fixtures() {
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
    let _run = stamp::start("big_face_down_hand_timing", std::env::var_os("BAV_SNAPSHOT_DIR").as_deref().map(std::path::Path::new));
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
