//! How strong the simulated player is (`sim::decide`, the policy every simulated round plays
//! by). Both tests are slow and print; they assert nothing. Run:
//! `cargo test --release --test player -- --ignored --nocapture` (`BAV_N`: rounds per board).

use balatro_advisor::{bench, data::GameData, engine::score::Board, save::RunState, sim};
use serde_json::Value;

/// One board per play style, on a standard deck, 8 cards in hand.
const BOARDS: &[(&str, &[&str])] = &[
    ("plain", &[]),
    ("pairs", &["j_jolly", "j_duo", "j_sly"]),
    ("trips", &["j_zany", "j_trio", "j_wily"]),
    ("straight", &["j_crazy", "j_order", "j_devious"]),
    ("straight_easy", &["j_crazy", "j_order", "j_shortcut", "j_four_fingers"]),
    ("flush", &["j_droll", "j_tribe", "j_crafty"]),
    ("flush_smeared", &["j_droll", "j_tribe", "j_smeared"]),
    ("faces", &["j_scary_face", "j_smiley", "j_photograph"]),
    ("high_card", &["j_joker", "j_cavendish", "j_misprint"]),
    ("held", &["j_baron", "j_shoot_the_moon", "j_raised_fist"]),
    ("discards", &["j_banner", "j_mystic_summit", "j_joker"]),
    ("ranks", &["j_fibonacci", "j_odd_todd", "j_even_steven"]),
];

/// Each board's round (4 hands, 3 or 1 discards) at the score the player of 3cfb122 (the
/// rulebook before dig plans) won half the time: fixed, so later players compare on the same
/// rounds.
const TARGETS: &[(&str, f64)] = &[
    ("plain_4h3d", 876.0),
    ("plain_4h1d", 676.0),
    ("pairs_4h3d", 8621.0),
    ("pairs_4h1d", 7808.0),
    ("trips_4h3d", 1152.0),
    ("trips_4h1d", 896.0),
    ("straight_4h3d", 8336.0),
    ("straight_4h1d", 896.0),
    ("straight_easy_4h3d", 21360.0),
    ("straight_easy_4h1d", 17281.0),
    ("flush_4h3d", 8896.0),
    ("flush_4h1d", 4779.0),
    ("flush_smeared_4h3d", 8372.0),
    ("flush_smeared_4h1d", 7812.0),
    ("faces_4h3d", 7739.0),
    ("faces_4h1d", 5714.0),
    ("high_card_4h3d", 7296.0),
    ("high_card_4h1d", 6225.0),
    ("held_4h3d", 5554.0),
    ("held_4h1d", 5150.0),
    ("discards_4h3d", 4752.0),
    ("discards_4h1d", 4563.0),
    ("ranks_4h3d", 12213.0),
    ("ranks_4h1d", 9091.0),
];

fn rounds() -> Vec<(String, Board, sim::RoundStart)> {
    let mut out = vec![];
    for (name, keys) in BOARDS {
        let b = bench::sample_board(keys);
        assert_eq!(b.jokers.len(), keys.len(), "{name}: unknown joker key");
        for (hands, discards) in [(4, 3), (4, 1)] {
            let key = format!("{name}_{hands}h{discards}d");
            let target = TARGETS.iter().find(|t| t.0 == key).unwrap().1;
            out.push((key, b.clone(), sim::RoundStart { hand: vec![], deck: bench::standard_deck(), hand_size: 8, hands, discards, scored: 0.0, target }));
        }
    }
    out
}

/// The player's win rate on each board's round (3cfb122: 50.1% on average, by construction;
/// with dig plans: 62.5%; with straight flush plans: 63.75%).
#[test]
#[ignore]
fn player_win_rates() {
    let n: usize = std::env::var("BAV_N").ok().and_then(|s| s.parse().ok()).unwrap_or(2000);
    let t0 = std::time::Instant::now();
    let mut sum = 0.0;
    let all = rounds();
    for (key, b, start) in &all {
        let w = sim::round_results(b, start, 0..n, 42).iter().filter(|r| r.won).count() as f64 / n as f64;
        sum += w;
        println!("{key}: {:.1}%", 100.0 * w);
    }
    println!("mean {:.2}% ({:.2?})", 100.0 * sum / all.len() as f64, t0.elapsed());
}

/// The player against the oracle (`sim::set_oracle`, `BAV_R` futures per alternative, default
/// 24): on each board's round, and on each private fixture's board (the round in progress, and
/// a fresh round at the score the player wins half the time). A gap well above its standard
/// error is a decision the player gets wrong; the oracle's choices are where to look.
#[test]
#[ignore]
fn player_against_oracle() {
    let n: usize = std::env::var("BAV_N").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let r: usize = std::env::var("BAV_R").ok().and_then(|s| s.parse().ok()).unwrap_or(24);
    let mut cases = rounds();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private");
    let mut files: Vec<_> = std::fs::read_dir(&dir).map(|e| e.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
    files.sort();
    for f in &files {
        let name = f.file_stem().unwrap().to_string_lossy().to_string();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let run: RunState = serde_json::from_value(v["state"].clone()).unwrap();
        let b = Board::from_run(&run, GameData::bundled());
        if let (true, Some(cb)) = (run.screen.in_blind(), &run.current_blind) {
            cases.push((format!("{name} live"), b.clone(), sim::RoundStart { hand: run.hand.clone(), deck: run.draw_pile.clone(), hand_size: run.hand_size, hands: run.hands_left, discards: run.discards_left, scored: cb.scored, target: cb.target }));
        }
        let mut fresh = b.clone();
        fresh.blind = Default::default();
        let mut deck = run.full_deck();
        for c in &mut deck {
            c.face_down = false;
            c.debuff = false;
        }
        deck.sort_by_key(|c| c.order_key());
        let mut start = sim::RoundStart { hand: vec![], deck, hand_size: run.hand_size, hands: run.round_hands, discards: run.round_discards, scored: 0.0, target: 1.0 };
        let (mut lo, mut hi) = (1.0f64, 1e12f64);
        for _ in 0..30 {
            start.target = (lo * hi).sqrt();
            let w = sim::round_results(&fresh, &start, 0..200, 7).iter().filter(|r| r.won).count();
            if w > 100 { lo = start.target } else { hi = start.target }
        }
        start.target = (lo * hi).sqrt();
        cases.push((format!("{name} fresh"), fresh, start));
    }
    let only = std::env::var("BAV_ONLY").ok();
    for (key, b, start) in cases {
        if only.as_ref().is_some_and(|o| !key.contains(o.as_str())) {
            continue;
        }
        let t0 = std::time::Instant::now();
        let a = sim::round_results(&b, &start, 0..n, 42);
        let ta = t0.elapsed();
        sim::set_oracle(r);
        let t1 = std::time::Instant::now();
        let o = sim::round_results(&b, &start, 0..n, 42);
        let to = t1.elapsed();
        sim::set_oracle(0);
        let d: Vec<f64> = a.iter().zip(&o).map(|(x, y)| y.won as u8 as f64 - x.won as u8 as f64).collect();
        let m = d.iter().sum::<f64>() / n as f64;
        let se = (d.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt() / (n as f64).sqrt();
        let win = |v: &[sim::RoundResult]| 100.0 * v.iter().filter(|x| x.won).count() as f64 / n as f64;
        println!("{key}: player {:.1}% oracle {:.1}%, gap {:+.1} ± {:.1} ({ta:.2?} vs {to:.2?})", win(&a), win(&o), 100.0 * m, 100.0 * se);
    }
}
