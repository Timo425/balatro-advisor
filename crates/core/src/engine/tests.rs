//! Engine tests. Expected scores are worked out by hand from the game rules
//! (state_events.lua `evaluate_play`, card.lua `calculate_joker`); each comment shows the arithmetic.

use super::*;
use crate::data::GameData;
use crate::model::{Card, Edition, Suit};

fn j(key: &str) -> Joker {
    Joker::from_key(key, GameData::bundled()).unwrap_or_else(|| panic!("unknown joker {key}"))
}

fn board(keys: &[&str]) -> Board {
    let mut b = Board::empty();
    b.jokers = keys.iter().map(|k| j(k)).collect();
    b
}

fn cards(s: &str) -> Vec<Card> {
    Card::parse_list(s).unwrap()
}

fn sc(b: &Board, played: &str, held: &str) -> f64 {
    score(b, &cards(played), &cards(held), &mut Unlucky, false).score
}

#[test]
fn no_jokers() {
    let b = Board::empty();
    assert_eq!(sc(&b, "AS", ""), 16.0); // (5 + 11) × 1
    assert_eq!(sc(&b, "KS KH", ""), 60.0); // (10 + 10 + 10) × 2
    assert_eq!(sc(&b, "KS KH 3D", ""), 60.0); // kicker doesn't score
    // Full House L1 40/4: (40 + 30 + 6) × 4
    assert_eq!(sc(&b, "KS KH KD 3C 3D", ""), 304.0);
}

#[test]
fn joker_order_matters_for_plus_then_times() {
    // +4 then ×3: (2 + 4) × 3 = 18 → 540; ×3 then +4: 2 × 3 + 4 = 10 → 300
    assert_eq!(sc(&board(&["j_joker", "j_cavendish"]), "KS KH", ""), 540.0);
    assert_eq!(sc(&board(&["j_cavendish", "j_joker"]), "KS KH", ""), 300.0);
}

#[test]
fn joker_editions() {
    let mut b = board(&["j_jolly"]);
    assert_eq!(sc(&b, "KS KH", ""), 300.0); // 30 × (2 + 8)
    b.jokers[0].edition = Some(Edition::Polychrome);
    assert_eq!(sc(&b, "KS KH", ""), 450.0); // 30 × (10 × 1.5): poly after the joker's own effect
    b.jokers[0].edition = Some(Edition::Holo);
    assert_eq!(sc(&b, "KS KH", ""), 600.0); // 30 × (2 + 10 + 8)
    b.jokers[0].edition = Some(Edition::Foil);
    assert_eq!(sc(&b, "KS KH", ""), 800.0); // 80 × 10
}

#[test]
fn card_enhancements_editions_seals() {
    let b = Board::empty();
    assert_eq!(sc(&b, "KS:glass KH", ""), 120.0); // 30 × (2 × 2)
    assert_eq!(sc(&b, "KS:mult KH:glass", ""), 360.0); // 30 × ((2 + 4) × 2)
    // as played, Glass first: 30 × (2 × 2 + 4); arranged, the Mult card goes first
    let (glass_first, mult_first) = (cards("KS:glass KH:mult"), cards("KS:mult KH:glass"));
    assert_eq!(score_as_played(&b, &glass_first, &[], &mut Unlucky, false).score, 240.0);
    let o = score(&b, &glass_first, &[], &mut Unlucky, false);
    assert_eq!((o.score, o.played_order), (360.0, Some(vec![1, 0])));
    assert_eq!(score(&b, &mult_first, &[], &mut Unlucky, false).played_order, None);
    assert_eq!(sc(&b, "KS:bonus KH", ""), 120.0); // 60 × 2
    assert_eq!(sc(&b, "KS:red KH", ""), 80.0); // (10 + 20 + 10) × 2
    assert_eq!(sc(&b, "KS:foil KH:holo", ""), 80.0 * 12.0); // (30 + 50) × (2 + 10)
    assert_eq!(sc(&b, "KS KH 5D:stone", ""), 160.0); // stone always scores +50
    assert_eq!(sc(&b, "KS:debuff KH", ""), 40.0); // still a pair, the debuffed K scores nothing
    let lucky = score(&b, &cards("KS:lucky KH"), &[], &mut Lucky, false).score;
    assert_eq!(lucky, 660.0); // 30 × (2 + 20)
}

#[test]
fn held_cards() {
    let b = Board::empty();
    assert_eq!(sc(&b, "KS KH", "5C:steel"), 90.0); // 30 × 3
    assert_eq!(sc(&board(&["j_mime"]), "KS KH", "5C:steel"), 135.0); // 30 × 2 × 1.5 × 1.5
    assert_eq!(sc(&board(&["j_mime"]), "KS KH", "5C"), 60.0); // nothing to retrigger
    assert_eq!(sc(&board(&["j_baron"]), "KS KH", "KD"), 90.0);
    assert_eq!(sc(&board(&["j_baron"]), "KS KH", "KD:debuff"), 60.0);
    assert_eq!(sc(&board(&["j_raised_fist"]), "KS KH", "7D 3C 9S"), 240.0); // +2×3 → 30 × 8
    assert_eq!(sc(&board(&["j_shoot_the_moon"]), "KS KH", "QD QC"), 30.0 * 28.0);
}

#[test]
fn cards_are_arranged_as_the_player_would() {
    // Shoot the Moon (+13 Mult a held Queen) and Baron (×1.5 a held King) score held cards
    // in hand order (state_events.lua `evaluate_play`, `G.hand.cards`): Queens first.
    // High Card A: 16 chips; Queens first (1 + 13 + 13) × 1.5 × 1.5 = 60.75 → 972;
    // Kings first 1 × 1.5 × 1.5 + 26 = 28.25 → 452
    let b = board(&["j_shoot_the_moon", "j_baron"]);
    let (play, kings_first, queens_first) = (cards("AS"), cards("KD KC QD QH"), cards("QD QH KD KC"));
    assert_eq!(score_as_played(&b, &play, &kings_first, &mut Unlucky, false).score, 452.0);
    assert_eq!(score_as_played(&b, &play, &queens_first, &mut Unlucky, false).score, 972.0);
    let o = score(&b, &play, &kings_first, &mut Unlucky, false);
    assert_eq!((o.score, o.held_order), (972.0, Some(vec![2, 3, 0, 1])));
    let o = score(&b, &play, &queens_first, &mut Unlucky, false);
    assert_eq!((o.score, o.held_order), (972.0, None));
    // A card's whole effect moves as one: a Red Seal Steel Queen with Shoot the Moon is
    // (m × 1.5 + 13) twice (its own ×1.5 first, then the joker's), m → 2.25m + 32.5, which
    // goes before a plain Steel card (×1.5 alone):
    // 2.25 + 32.5 = 34.75 → × 1.5 = 52.125 → 16 × 52.125 = 834 (the other way: 1.5 → 35.875 → 574)
    let b = board(&["j_shoot_the_moon"]);
    let held = cards("5C:steel QD:steel:red");
    assert_eq!(score_as_played(&b, &play, &held, &mut Unlucky, false).score, 574.0);
    let o = score(&b, &play, &held, &mut Unlucky, true);
    assert_eq!((o.score, o.held_order), (834.0, Some(vec![1, 0])));
    // the trace follows the order applied
    assert_eq!(o.trace.last().map(|s| s.mult), Some(52.125));
    // Photograph reads which face card comes first: the Mult card first takes its ×2 and the
    // Glass card's ×2 after it: (2 + 4) × 2 × 2 = 24 → 720 (as given, Glass first: 360)
    let b = board(&["j_photograph"]);
    let glass_first = cards("KS:glass KH:mult");
    assert_eq!(score_as_played(&b, &glass_first, &[], &mut Unlucky, false).score, 360.0);
    let o = score(&b, &glass_first, &[], &mut Unlucky, false);
    assert_eq!((o.score, o.played_order), (720.0, Some(vec![1, 0])));
    // Raised Fist goes on the last held card of the lowest rank, wherever it ends up: with
    // Baron, K K held, the +20 is always on the second King: 1 × 1.5 × 1.5 + 20 = 22.25 → 356
    // (counted on the King it was worked out on and moved first: 516, which no order gives)
    let b = board(&["j_baron", "j_raised_fist"]);
    let o = score(&b, &play, &cards("KD KC"), &mut Unlucky, false);
    assert_eq!((o.score, o.held_order), (356.0, None));
    // An order is judged on the whole hand: Hanging Chad retriggers the first card, so its
    // chips move with the order, and Jolly (+8) and Gros Michel (+15) come after the cards.
    // Glass Foil King first: 200 chips × (2 × 2 × 2 × 2 + 10 = 26 → + 23) = 9,800; the Holo
    // King first leads after the cards (100 × 64) but loses after the jokers (100 × 87 = 8,700)
    let b = board(&["j_hanging_chad", "j_jolly", "j_gros_michel"]);
    let given = cards("KS:glass:foil KH:holo");
    let o = score(&b, &given, &[], &mut Unlucky, false);
    assert_eq!((o.score, o.played_order), (9800.0, None));
}

#[test]
fn retriggers() {
    assert_eq!(sc(&board(&["j_hanging_chad"]), "KS KH", ""), 100.0); // KS ×3: (10 + 30 + 10) × 2
    assert_eq!(sc(&board(&["j_sock_and_buskin"]), "KS KH", ""), 100.0); // both faces ×2
    let mut b = board(&["j_dusk"]);
    b.hands_left = 1; // this is the last hand
    assert_eq!(sc(&b, "KS KH", ""), 100.0);
    b.hands_left = 2;
    assert_eq!(sc(&b, "KS KH", ""), 60.0);
    // Hiker adds +5 perma after the card's own chips, so the retrigger already has it
    assert_eq!(sc(&board(&["j_hiker"]), "KS:red KH", ""), 90.0); // (10 + 10 + 15 + 10) × 2
}

#[test]
fn blueprint_and_brainstorm() {
    assert_eq!(sc(&board(&["j_blueprint", "j_cavendish"]), "KS KH", ""), 540.0); // 2 × 3 × 3
    assert_eq!(sc(&board(&["j_cavendish", "j_brainstorm"]), "KS KH", ""), 540.0);
    assert_eq!(sc(&board(&["j_cavendish", "j_blueprint"]), "KS KH", ""), 180.0); // nothing to its right
    assert_eq!(sc(&board(&["j_blueprint", "j_blueprint", "j_joker"]), "KS KH", ""), 30.0 * 14.0);
}

#[test]
fn rule_changers() {
    let b = Board::empty();
    assert_eq!(sc(&b, "2H 7H 9H JH 3S", ""), 15.0); // high card J
    // Flush L1 35/4 with Four Fingers: (35 + 2 + 7 + 9 + 10) × 4
    assert_eq!(sc(&board(&["j_four_fingers"]), "2H 7H 9H JH 3S", ""), 252.0);
    // Straight L1 30/4 with Shortcut: (30 + 30) × 4
    assert_eq!(sc(&board(&["j_shortcut"]), "2S 4H 6D 8C 10D", ""), 240.0);
    assert_eq!(sc(&board(&["j_smeared"]), "2H 7D 9H JD KH", ""), (35.0 + 38.0) * 4.0);
    assert_eq!(sc(&board(&["j_splash"]), "KS KH 3D", ""), 66.0); // kicker scores too
    // Pareidolia: every card is a face
    assert_eq!(sc(&board(&["j_pareidolia", "j_smiley"]), "5S 5H", ""), 20.0 * 12.0);
}

#[test]
fn per_card_jokers() {
    assert_eq!(sc(&board(&["j_photograph"]), "KS KH", ""), 120.0); // first face ×2
    assert_eq!(sc(&board(&["j_scholar"]), "AS AH", ""), 72.0 * 10.0); // (10 + 31 + 31) × (2 + 4 + 4)
    let mut b = board(&["j_idol"]);
    b.idol = Some((13, Suit::Spades));
    assert_eq!(sc(&b, "KS KH", ""), 120.0);
    let mut b = board(&["j_ancient"]);
    b.ancient_suit = Some(Suit::Hearts);
    assert_eq!(sc(&b, "KH KD", ""), 90.0);
    assert_eq!(sc(&board(&["j_greedy_joker"]), "KD KH", ""), 30.0 * 5.0);
    assert_eq!(sc(&board(&["j_even_steven", "j_odd_todd"]), "8S 8H", ""), 26.0 * 10.0);
}

#[test]
fn scaling_jokers_update_before_scoring() {
    let mut b = board(&["j_green_joker"]);
    b.jokers[0].mult = 3.0;
    assert_eq!(sc(&b, "KS KH", ""), 180.0); // +1 before scoring → 30 × (2 + 4)
    let mut b = board(&["j_ride_the_bus"]);
    b.jokers[0].mult = 5.0;
    assert_eq!(sc(&b, "KS KH", ""), 60.0); // a face resets it to 0
    assert_eq!(sc(&b, "5S 5H", ""), 20.0 * 8.0); // 5 + 1
    let b = board(&["j_square"]);
    assert_eq!(sc(&b, "KS KH 3D 4C", ""), (30.0 + 4.0) * 2.0);
}

#[test]
fn state_dependent_jokers() {
    assert_eq!(sc(&board(&["j_abstract", "j_credit_card", "j_egg"]), "KS KH", ""), 30.0 * 11.0);
    let mut b = board(&["j_blue_joker"]);
    b.deck_remaining = 40;
    assert_eq!(sc(&b, "KS KH", ""), 220.0);
    let mut b = board(&["j_card_sharp"]);
    assert_eq!(sc(&b, "KS KH", ""), 60.0);
    b.levels[HandType::Pair as usize].played_this_round = 1;
    assert_eq!(sc(&b, "KS KH", ""), 180.0);
    let mut b = board(&["j_supernova"]);
    b.levels[HandType::Pair as usize].played = 4;
    assert_eq!(sc(&b, "KS KH", ""), 210.0); // counts this hand: 2 + 5
    let mut b = board(&["j_loyalty_card"]);
    b.hands_played = 5; // the 6th hand since it was bought
    assert_eq!(sc(&b, "KS KH", ""), 240.0);
    b.hands_played = 4;
    assert_eq!(sc(&b, "KS KH", ""), 60.0);
    let mut b = board(&["j_throwback"]);
    b.skips = 4;
    assert_eq!(sc(&b, "KS KH", ""), 120.0);
    let mut b = board(&["j_mystic_summit"]);
    b.discards_left = 0;
    assert_eq!(sc(&b, "KS KH", ""), 30.0 * 17.0);
    // Stencil: 5 slots, 1 joker → ×5
    assert_eq!(sc(&board(&["j_stencil"]), "KS KH", ""), 300.0);
    // Swashbuckler: + Jolly's sell value ($1)
    assert_eq!(sc(&board(&["j_swashbuckler", "j_jolly"]), "KS KH", ""), 30.0 * 11.0);
    let mut b = board(&["j_bootstraps", "j_bull"]);
    b.dollars = 12.0;
    assert_eq!(sc(&b, "KS KH", ""), 54.0 * 6.0); // +2×2 mult, +2×12 chips
    // Duo counts a pair inside a full house
    assert_eq!(sc(&board(&["j_duo"]), "KS KH KD 3C 3D", ""), 608.0);
    // Baseball: ×1.5 for each Uncommon
    assert_eq!(sc(&board(&["j_four_fingers", "j_baseball", "j_mime"]), "KS KH", ""), 135.0);
}

#[test]
fn bosses_and_decks() {
    let mut b = Board::empty();
    b.blind.key = "bl_flint".into();
    assert_eq!(sc(&b, "KS KH", ""), 25.0); // base halved to 5 × 1
    b.blind.key = "bl_psychic".into();
    let o = score(&b, &cards("KS KH"), &[], &mut Unlucky, false);
    assert!(o.debuffed_hand && o.score == 0.0);
    b.blind.disabled = true;
    assert_eq!(sc(&b, "KS KH", ""), 60.0);
    let mut b = Board::empty();
    b.plasma = true;
    assert_eq!(sc(&b, "KS KH", ""), 256.0); // (30 + 2) / 2 = 16 each
    let mut b = Board::empty();
    b.observatory = true;
    b.planets_held = vec![HandType::Pair];
    assert_eq!(sc(&b, "KS KH", ""), 90.0);
    let mut b = Board::empty();
    b.blind.key = "bl_arm".into();
    b.levels[HandType::Pair as usize] = Level::base(HandType::Pair).with_level(3);
    assert_eq!(sc(&b, "KS KH", ""), (25.0 + 20.0) * 3.0); // played at level 2
}

#[test]
fn trace_ends_at_the_score() {
    let b = board(&["j_joker", "j_cavendish"]);
    let o = score(&b, &cards("KS KH"), &[], &mut Unlucky, true);
    let last = o.trace.last().unwrap();
    assert_eq!(last.chips * last.mult, o.score);
    assert!(o.trace.iter().any(|s| s.source == "Cavendish"));
}

#[test]
fn jokers_grow_from_shop_events_by_their_game_data() {
    // card.lua: Red Card +extra (3) Mult per booster pack skipped, Flash Card +extra (2) per
    // reroll; the amounts come from data/game.json, nothing else grows from them
    for (key, ev, per) in [("j_red_card", RunEvent::SkipPack, 3.0), ("j_flash", RunEvent::Reroll, 2.0)] {
        let mut x = j(key);
        assert_eq!(x.mult_from(ev), per, "{key}");
        let other = RunEvent::ALL.into_iter().find(|&e| e != ev).unwrap();
        assert_eq!(x.mult_from(other), 0.0, "{key} on {other:?}");
        x.grow_from(ev, 2.0);
        assert_eq!(x.mult, 2.0 * per);
        x.debuff = true;
        assert_eq!(x.mult_from(ev), 0.0, "a debuffed {key} doesn't grow");
    }
    for key in ["j_joker", "j_blueprint", "j_constellation", "j_green_joker", "j_throwback"] {
        assert!(RunEvent::ALL.iter().all(|&e| j(key).mult_from(e) == 0.0), "{key}");
    }
}

#[test]
fn a_skipped_blind_is_counted_where_throwback_scores_from_it() {
    // G.GAME.skips +1 per skipped blind; Throwback ×(1 + 0.25 per skip)
    let mut b = board(&["j_throwback"]);
    assert!(b.changes_with(RunEvent::SkipBlind) && !board(&["j_joker"]).changes_with(RunEvent::SkipBlind));
    assert_eq!(sc(&b, "KS KH", ""), 60.0);
    b.after(RunEvent::SkipBlind, 2.0);
    assert_eq!(sc(&b, "KS KH", ""), 60.0 * 1.5);
    let mut r = board(&["j_red_card", "j_red_card"]);
    r.after(RunEvent::SkipPack, 1.0);
    assert!(r.jokers.iter().all(|x| x.mult == 3.0) && r.changes_with(RunEvent::SkipPack));
}

#[test]
fn the_hooks_discard_uses_no_discard_and_burnt_joker_skips_it() {
    // state_events.lua `G.FUNCS.discard_cards_from_highlighted` with `hook`: no discard used;
    // card.lua Burnt Joker checks `not context.hook`; the rest react as to any discard
    let mut b = board(&["j_burnt", "j_green_joker"]);
    b.jokers[1].mult = 5.0;
    b.discards_left = 3;
    let before = b.levels[HandType::Pair as usize].level;
    b.discard_by(&cards("KS KH"), true);
    assert_eq!((b.discards_used, b.discards_left), (0, 3));
    assert_eq!(b.levels[HandType::Pair as usize].level, before, "Burnt Joker levelled the Hook's discard");
    assert_eq!(b.jokers[1].mult, 4.0, "Green Joker loses Mult on the Hook's discard too");
    // the first discard of your own then still levels with Burnt Joker
    b.discard(&cards("QS QH"));
    assert_eq!((b.discards_used, b.discards_left), (1, 2));
    assert_eq!(b.levels[HandType::Pair as usize].level, before + 1);
}
