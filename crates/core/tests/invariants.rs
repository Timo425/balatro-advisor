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
fn a_discard_changes_the_board_as_the_game_does() {
    // state_events.lua discard_cards_from_highlighted, card.lua pre_discard / discard contexts
    let cards = |s: &str| Card::parse_list(s).unwrap();
    let get = |b: &engine::Board, k: &str| b.jokers.iter().find(|j| j.key == k).cloned().unwrap();
    let mut b = sample_board(&["j_green_joker", "j_ramen", "j_castle", "j_hit_the_road", "j_yorick", "j_blueprint", "j_burnt"]);
    b.castle_suit = Some(balatro_advisor::model::Suit::Spades);
    b.jokers[0].mult = 5.0;
    b.jokers[4].yorick_discards = 2.0;
    let mut jack = cards("JS JH:stone 4S:wild 9D JC");
    jack[4].debuff = true;
    assert_eq!(b.discard(&jack), 0.0);
    assert_eq!(get(&b, "j_green_joker").mult, 4.0, "Green Joker: -1 once a discard, not a card");
    assert!((get(&b, "j_ramen").x_mult - 1.95).abs() < 1e-9, "Ramen: -0.01 a card");
    assert_eq!(get(&b, "j_castle").extra.chips, 6.0, "Castle: a Spade and a Wild card, not the debuffed Jack");
    assert_eq!(get(&b, "j_hit_the_road").x_mult, 1.5, "Hit the Road: a Jack, not a Stone or debuffed one");
    let y = get(&b, "j_yorick");
    assert_eq!((y.x_mult, y.yorick_discards), (2.0, 20.0), "Yorick: a step every 23 cards");
    // the round's first discard: Burnt Joker and the Blueprint copying it each level the
    // discarded cards' hand (from level 1)
    let h = engine::hand::detect(&jack, b.rule_flags()).hand;
    assert_eq!(b.levels[h as usize].level, 3, "Burnt Joker and its copy level {h:?} on the first discard");
    assert_eq!((b.discards_used, b.discards_left), (1, 2));
    b.discard(&cards("2D"));
    assert_eq!(b.levels[h as usize].level, 3, "only the round's first discard levels a hand");
    assert_eq!(get(&b, "j_green_joker").mult, 3.0);

    // Ramen is eaten once it would reach x1; Green Joker stops at 0
    let mut r = sample_board(&["j_ramen", "j_green_joker"]);
    r.jokers[0].x_mult = 1.02;
    r.discard(&cards("2S 3S 4S"));
    assert!(r.jokers.iter().all(|j| j.key != "j_ramen"));
    r.discard(&cards("2S"));
    assert_eq!(get(&r, "j_green_joker").mult, 0.0);

    // Trading Card: $3 for a first discard of one card, which is destroyed
    let mut t = sample_board(&["j_trading", "j_blueprint"]);
    let deck = t.playing_cards;
    assert_eq!(engine::discard_money(&t, &cards("2S 3S")), 0.0);
    assert_eq!(t.discard(&cards("2S")), 3.0, "a copy doesn't pay");
    assert_eq!((t.playing_cards, t.dollars), (deck - 1, 3.0));
    assert_eq!(t.discard(&cards("3S")), 0.0, "only the round's first discard");
}

#[test]
fn a_hand_leaves_its_jokers_for_the_next() {
    // the joker state the hand changed carries on, then the `after` context
    let mut b = sample_board(&["j_green_joker", "j_ice_cream", "j_selzer", "j_ride_the_bus"]);
    b.jokers[2].extra.n = 1.0;
    let played = Card::parse_list("2S 2H").unwrap();
    let o = engine::score(&b, &played, &[], &mut Unlucky, false);
    b.after_hand(&o);
    assert_eq!(b.jokers[0].mult, 1.0, "Green Joker +1 a hand");
    assert_eq!(b.jokers[1].extra.chips, 95.0, "Ice Cream -5 a hand");
    assert!(b.jokers.iter().all(|j| j.key != "j_selzer"), "Seltzer's last use");
    assert_eq!(b.jokers[2].mult, 1.0, "Ride the Bus +1 without a face");
    assert_eq!(b.levels[engine::HandType::Pair as usize].played_this_round, 1);
    assert_eq!(b.hands_played, 1);
}

#[test]
fn a_new_round_resets_what_lasts_a_round() {
    // state_events.lua new_round; card.lua end_of_round: Hit the Road back to x1
    let mut b = sample_board(&["j_hit_the_road", "j_green_joker"]);
    b.discard(&Card::parse_list("JS").unwrap());
    let o = engine::score(&b, &Card::parse_list("2S 2H").unwrap(), &[], &mut Unlucky, false);
    b.after_hand(&o);
    b.new_round("bl_small");
    assert_eq!((b.jokers[0].x_mult, b.discards_used, b.blind.key.as_str()), (1.0, 0, "bl_small"));
    assert!(b.levels.iter().all(|l| l.played_this_round == 0));
    assert_eq!(b.jokers[1].mult, 1.0, "Green Joker keeps its Mult (0 after the discard, +1 a hand)");
    // a debuffed joker doesn't take part (card.lua calculate_joker returns at once)
    let mut d = sample_board(&["j_hit_the_road"]);
    d.jokers[0].x_mult = 2.0;
    d.jokers[0].debuff = true;
    d.new_round("bl_small");
    assert_eq!(d.jokers[0].x_mult, 2.0);
}

#[test]
fn delayed_gratification_pays_for_discards_never_used() {
    // card.lua calculate_dollar_bonus: discards_used == 0, extra per discard left
    let mut b = sample_board(&["j_delayed_grat"]);
    b.discards_left = 3;
    assert_eq!(engine::won_round_money(&b), 6.0);
    b.discard(&Card::parse_list("2S").unwrap());
    assert_eq!(engine::won_round_money(&b), 0.0);
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
