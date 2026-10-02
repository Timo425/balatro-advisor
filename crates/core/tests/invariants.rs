//! Properties that must hold however the engine changes ("must hold" tests).

use balatro_advisor::bench::{sample_board, standard_deck};
use balatro_advisor::data::GameData;
use balatro_advisor::engine::{self, Joker, Rng, Unlucky};
use balatro_advisor::model::Card;
use balatro_advisor::sim::{self, RoundStart};

/// Random 1–5 card plays with 3 held cards, from a shuffled standard deck.
fn random_plays(n: usize, seed: u64) -> Vec<(Vec<Card>, Vec<Card>)> {
    let mut rng = Rng::new(seed);
    let mut deck = standard_deck();
    (0..n)
        .map(|i| {
            sim::shuffle(&mut deck, &mut rng);
            let k = 1 + i % 5;
            (deck[..k].to_vec(), deck[k..k + 3].to_vec())
        })
        .collect()
}

#[test]
fn a_plus_mult_joker_never_lowers_a_score() {
    let data = GameData::bundled();
    let joker = Joker::from_key("j_joker", data).unwrap();
    for keys in [&["j_droll"][..], &["j_cavendish", "j_scholar"], &["j_blueprint", "j_duo", "j_baron"]] {
        let b = sample_board(keys);
        let mut with = b.clone();
        with.jokers.push(joker.clone());
        for (played, held) in random_plays(300, 7) {
            let a = engine::score(&b, &played, &held, &mut Unlucky, false).score;
            let c = engine::score(&with, &played, &held, &mut Unlucky, false).score;
            assert!(c >= a, "{keys:?}: +Mult joker lowered {a} → {c} for {:?}", played.iter().map(Card::label).collect::<Vec<_>>());
        }
    }
}

#[test]
fn chip_jokers_score_the_same_in_any_slot() {
    let mut b1 = sample_board(&["j_banner", "j_droll", "j_stuntman"]);
    b1.discards_left = 2;
    let mut b2 = sample_board(&["j_droll", "j_stuntman", "j_banner"]);
    b2.discards_left = 2;
    for (played, held) in random_plays(300, 11) {
        let a = engine::score(&b1, &played, &held, &mut Unlucky, false).score;
        let c = engine::score(&b2, &played, &held, &mut Unlucky, false).score;
        assert_eq!(a, c, "chips depended on joker order");
    }
}

#[test]
fn plus_mult_before_x_mult_is_never_worse() {
    // Joker (+4 Mult) left of Cavendish (×3) gets multiplied; right of it doesn't.
    let before = sample_board(&["j_joker", "j_cavendish"]);
    let after = sample_board(&["j_cavendish", "j_joker"]);
    for (played, held) in random_plays(300, 13) {
        let a = engine::score(&before, &played, &held, &mut Unlucky, false).score;
        let c = engine::score(&after, &played, &held, &mut Unlucky, false).score;
        assert!(a >= c);
    }
}

#[test]
fn a_flush_multiplier_beats_a_pair_bonus_on_a_flush_board() {
    // Same rounds, same seeds: Droll + The Tribe (×2 flushes) must out-score Droll + Jolly (+8 on pairs).
    let start = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1e300 };
    let tribe = sim::round_odds(&sample_board(&["j_droll", "j_tribe"]), &start, 300, 5).1.mean;
    let jolly = sim::round_odds(&sample_board(&["j_droll", "j_jolly"]), &start, 300, 5).1.mean;
    assert!(tribe > jolly, "Tribe {tribe} vs Jolly {jolly}");
}

#[test]
fn simulations_are_reproducible() {
    let b = sample_board(&["j_droll", "j_lusty_joker", "j_banner"]);
    let start = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 3000.0 };
    let a = sim::round_odds(&b, &start, 200, 9);
    let c = sim::round_odds(&b, &start, 200, 9);
    assert_eq!(a.0, c.0);
    assert_eq!(a.1.mean, c.1.mean);
}

#[test]
fn burning_discards_pays_only_when_the_board_scores_more_without_them() {
    // Decided by the engine's scoring, not by joker names: Mystic Summit pays with no
    // discards left, Banner per discard kept, a plain joker doesn't care.
    let pair = Card::parse_list("AS AH").unwrap();
    let burn = |keys: &[&str]| {
        let mut b = sample_board(keys);
        b.discards_left = 3;
        sim::scores_more_without_discards(&b, &pair)
    };
    assert!(burn(&["j_mystic_summit"]));
    assert!(!burn(&["j_banner"]));
    assert!(!burn(&["j_joker"]));
}

#[test]
fn discard_money_follows_the_game() {
    // card.lua, discard context: Mail-In pays per card of its rank (get_id: not Stone, not
    // debuffed), Faceless Joker for 3+ faces (is_face: Pareidolia makes any card one); a
    // Blueprint copy pays too.
    let cards = |s: &str| Card::parse_list(s).unwrap();
    let mut b = sample_board(&["j_mail"]);
    b.mail_rank = Some(4);
    assert_eq!(engine::discard_money(&b, &cards("4S 4H 9C")), 10.0);
    let mut c = cards("4S:stone 4H");
    c[1].debuff = true;
    assert_eq!(engine::discard_money(&b, &c), 0.0, "Stone and debuffed cards don't pay");
    let mut bp = sample_board(&["j_blueprint", "j_mail"]);
    bp.mail_rank = Some(4);
    assert_eq!(engine::discard_money(&bp, &cards("4S")), 10.0, "a Blueprint copy pays too");
    let f = sample_board(&["j_faceless"]);
    assert_eq!(engine::discard_money(&f, &cards("KS QH JD")), 5.0);
    assert_eq!(engine::discard_money(&f, &cards("KS QH")), 0.0);
    assert_eq!(engine::discard_money(&f, &cards("KS QH JD:stone")), 0.0, "a Stone card isn't a face");
    assert_eq!(engine::discard_money(&sample_board(&["j_faceless", "j_pareidolia"]), &cards("2S 3H 4D")), 5.0);
}
