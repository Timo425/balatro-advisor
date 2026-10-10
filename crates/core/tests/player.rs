//! How strong the simulated player is (`sim::decide`, the policy every simulated round plays
//! by). Both tests are slow and print; they assert nothing. Run:
//! `cargo test --release --test player -- --ignored --nocapture` (`BAV_N`: rounds per board).

use balatro_advisor::{bench, data::GameData, engine::score::Board, model::Card, save::RunState, sim};
use serde_json::Value;

/// One board per play style, 8 cards in hand: its jokers, whether its deck is `ENHANCED` (else
/// the standard 52) and its boss ("" for none). The first twelve are plain play styles; the
/// rest have what those lack and real runs have: jokers that grow during the round, a few
/// strong cards among plain ones, and a boss that debuffs cards.
const BOARDS: &[(&str, &[&str], bool, &str)] = &[
    ("plain", &[], false, ""),
    ("pairs", &["j_jolly", "j_duo", "j_sly"], false, ""),
    ("trips", &["j_zany", "j_trio", "j_wily"], false, ""),
    ("straight", &["j_crazy", "j_order", "j_devious"], false, ""),
    ("straight_easy", &["j_crazy", "j_order", "j_shortcut", "j_four_fingers"], false, ""),
    ("flush", &["j_droll", "j_tribe", "j_crafty"], false, ""),
    ("flush_smeared", &["j_droll", "j_tribe", "j_smeared"], false, ""),
    ("faces", &["j_scary_face", "j_smiley", "j_photograph"], false, ""),
    ("high_card", &["j_joker", "j_cavendish", "j_misprint"], false, ""),
    ("held", &["j_baron", "j_shoot_the_moon", "j_raised_fist"], false, ""),
    ("discards", &["j_banner", "j_mystic_summit", "j_joker"], false, ""),
    ("ranks", &["j_fibonacci", "j_odd_todd", "j_even_steven"], false, ""),
    ("trousers", &["j_trousers", "j_trousers", "j_mad"], false, ""),
    ("growers", &["j_green_joker", "j_ride_the_bus", "j_runner"], false, ""),
    ("enhanced", &["j_jolly", "j_duo", "j_joker"], true, ""),
    ("trousers_goad", &["j_trousers", "j_trousers", "j_joker"], true, "bl_goad"),
    ("flush_club", &["j_droll", "j_tribe", "j_crafty"], false, "bl_club"),
];

/// A deck as a run has it by Ante 3 or 4: the standard 52 with these cards changed (Bonus 3s,
/// a few Glass, Steel and Mult cards, seals) and two more 3s.
const ENHANCED: &[&str] = &[
    "3S:bonus", "3H:bonus:goldseal", "3C:bonus", "3D:bonus", "KH:glass", "7C:glass", "9D:glass", "QS:steel", "JD:steel",
    "5H:mult", "TC:mult", "8D:goldseal", "AS:blue",
];
const ENHANCED_EXTRA: &[&str] = &["3H", "3C"];

fn enhanced_deck() -> Vec<Card> {
    let mut deck = bench::standard_deck();
    for t in ENHANCED {
        let c = Card::parse(t).unwrap();
        *deck.iter_mut().find(|d| d.rank == c.rank && d.suit == c.suit).unwrap() = c;
    }
    deck.extend(ENHANCED_EXTRA.iter().map(|t| Card::parse(t).unwrap()));
    deck
}

/// Each board's round (4 hands, 3 or 1 discards) at the score the player of 3cfb122 (the
/// rulebook before dig plans) won half the time (the boards added later: the player of their
/// day): fixed, so later players compare on the same rounds.
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
    // the boards added 2026-10-09: the half points of the player at d4195a0 (`player_targets`)
    ("trousers_4h3d", 3672.0),
    ("trousers_4h1d", 2884.0),
    ("growers_4h3d", 1726.0),
    ("growers_4h1d", 1474.0),
    ("enhanced_4h3d", 7450.0),
    ("enhanced_4h1d", 5991.0),
    ("trousers_goad_4h3d", 3600.0),
    ("trousers_goad_4h1d", 2918.0),
    ("flush_club_4h3d", 8808.0),
    ("flush_club_4h1d", 4788.0),
    // one hand and 3 discards (The Needle), added 2026-10-10: the half points of the first
    // player that digs before its last hand (before it, a one-hand round was its first deal)
    ("plain_1h3d", 316.0),
    ("pairs_1h3d", 3096.0),
    ("trips_1h3d", 8544.0),
    ("straight_1h3d", 8352.0),
    ("straight_easy_1h3d", 8640.0),
    ("flush_1h3d", 4368.0),
    ("flush_smeared_1h3d", 2380.0),
    ("faces_1h3d", 4788.0),
    ("high_card_1h3d", 2691.0),
    ("held_1h3d", 2844.0),
    ("discards_1h3d", 1817.0),
    ("ranks_1h3d", 5868.0),
    ("trousers_1h3d", 1208.0),
    ("growers_1h3d", 470.0),
    ("enhanced_1h3d", 2976.0),
    ("trousers_goad_1h3d", 1248.0),
    ("flush_club_1h3d", 4368.0),
];

/// Each board's rounds; a board without a target yet gets 0 (`player_targets` finds it).
fn rounds() -> Vec<(String, Board, sim::RoundStart)> {
    let mut out = vec![];
    for (name, keys, enhanced, boss) in BOARDS {
        let mut b = bench::sample_board(keys);
        assert_eq!(b.jokers.len(), keys.len(), "{name}: unknown joker key");
        b.new_round(boss);
        // the boss's debuffs and hand size, as advise's blind_from_start sets them
        let rules = sim::RoundRules::for_blind(boss);
        let mut deck = if *enhanced { enhanced_deck() } else { bench::standard_deck() };
        let f = b.rule_flags();
        rules.apply(&mut deck, f.smeared, f.pareidolia);
        // a round of 4 hands with 3 or 1 discards, and The Needle's one hand with 3
        for (hands, discards) in [(4, 3), (4, 1), (1, 3)] {
            let key = format!("{name}_{hands}h{discards}d");
            let target = TARGETS.iter().find(|t| t.0 == key).map_or(0.0, |t| t.1);
            out.push((key, b.clone(), sim::RoundStart { hand: vec![], deck: deck.clone(), hand_size: 8 + rules.hand_size_delta, hands, discards, scored: 0.0, target }));
        }
    }
    out
}

/// The score the player wins about half the time at, over rounds `0..n` from `seed`.
fn half_point(b: &Board, start: &sim::RoundStart, n: usize, seed: u64) -> f64 {
    let mut start = start.clone();
    let (mut lo, mut hi) = (1.0f64, 1e12f64);
    for _ in 0..30 {
        start.target = (lo * hi).sqrt();
        let w = sim::round_results(b, &start, 0..n, seed).iter().filter(|r| r.won).count();
        if 2 * w > n { lo = start.target } else { hi = start.target }
    }
    (lo * hi).sqrt()
}

/// Prints each board's half point by the player now (`BAV_N` rounds, default 2000;
/// `BAV_ONLY`): how a new board's `TARGETS` are set.
#[test]
#[ignore]
fn player_targets() {
    let n: usize = std::env::var("BAV_N").ok().and_then(|s| s.parse().ok()).unwrap_or(2000);
    let only = std::env::var("BAV_ONLY").ok();
    for (key, b, start) in rounds() {
        if only.as_ref().is_some_and(|o| !o.split(',').filter(|o| !o.is_empty()).any(|o| key.contains(o))) {
            continue;
        }
        println!("(\"{key}\", {:.0}.0),", half_point(&b, &start, n, 42));
    }
}

/// The player's win rate on each board's round (3cfb122: 50.1% on average, by construction;
/// with dig plans: 62.5%; with straight flush plans: 63.75%).
#[test]
#[ignore]
fn player_win_rates() {
    let n: usize = std::env::var("BAV_N").ok().and_then(|s| s.parse().ok()).unwrap_or(2000);
    let t0 = std::time::Instant::now();
    let all = rounds();
    let mut wins = vec![];
    for (key, b, start) in &all {
        let t = std::time::Instant::now();
        let w = sim::round_results(b, start, 0..n, 42).iter().filter(|r| r.won).count() as f64 / n as f64;
        wins.push(w);
        println!("{key}: {:.1}% ({:.2?})", 100.0 * w, t.elapsed());
    }
    let mean = |keep: &dyn Fn(&str) -> bool| {
        let w: Vec<f64> = all.iter().zip(&wins).filter(|(r, _)| keep(&r.0)).map(|(_, w)| *w).collect();
        100.0 * w.iter().sum::<f64>() / w.len().max(1) as f64
    };
    // the plain play styles' 4-hand rounds stay comparable with the history above
    let plain = |k: &str| BOARDS[..12].iter().any(|b| k.starts_with(&format!("{}_4h", b.0)));
    println!(
        "mean {:.2}% (plain styles {:.2}%, the boards added since {:.2}%, one hand {:.2}%) ({:.2?})",
        mean(&|_| true),
        mean(&plain),
        mean(&|k| !plain(k) && !k.ends_with("_1h3d")),
        mean(&|k| k.ends_with("_1h3d")),
        t0.elapsed()
    );
}

/// The player against the oracle (`sim::set_oracle`, `BAV_R` futures per alternative, default
/// 24): on each board's round, and on each private fixture's board (the round in progress, and
/// a fresh round at the score the player wins half the time). A gap well above its standard
/// error is a decision the player gets wrong; the oracle's choices are where to look.
/// `BAV_ONLY=a,b`: the cases whose name contains any of these (the long run in parts).
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
            cases.push((format!("{name} live"), b.clone(), sim::RoundStart { hand: run.hand.clone(), deck: run.draw_pile.clone(), hand_size: run.round_hand_size(), hands: run.hands_left, discards: run.discards_left, scored: cb.scored, target: cb.target }));
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
        start.target = half_point(&fresh, &start, 200, 7);
        cases.push((format!("{name} fresh"), fresh, start));
    }
    let only = std::env::var("BAV_ONLY").ok();
    for (key, b, start) in cases {
        if only.as_ref().is_some_and(|o| !o.split(',').filter(|o| !o.is_empty()).any(|o| key.contains(o))) {
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
