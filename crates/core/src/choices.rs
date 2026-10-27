//! What the owner chose between two saves of a run in a row: a hand played or a discard (by the
//! cards' places in the hand as saved), a buy, a sale, a reroll or leaving the shop, a pick or a
//! skip in a pack, playing or skipping a blind, the jokers' order. The saves are the live page's history
//! (`history/<seed>/`), so each choice was made with the advice for the state before it on
//! screen.
//!
//! Plays come from the hands log (`handlog::pair`). A discard is followed the same way, by the
//! cards' ids: the ones that left the hand and reached the discard pile. The rest is read from
//! what changed between the two saves: the game doesn't save after every click, so two choices
//! between two saves (a buy and a reroll, two buys) can't be told apart and are counted as such
//! (`Unclear`), never guessed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::data::GameData;
use crate::handlog::{self, Pairing};
use crate::save::{self, CardIds, RunState, Screen};

/// A choice made between two saves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Choice {
    /// A hand played: the cards' places in the hand as saved, sorted
    Play { positions: Vec<usize>, cards: Vec<String> },
    Discard { positions: Vec<usize>, cards: Vec<String> },
    /// Bought from the shop: a joker (with the joker sold to make room, if one went in the same
    /// step), a pack, a voucher or a card from the card row. `kind` is the shop ranking's
    /// (`advise::ShopOption::kind`): joker, pack, voucher, planet, tarot, card
    Buy { item: String, key: String, name: String, sold: Option<String> },
    Sell { key: String, name: String },
    Reroll,
    /// Left the shop for the next blind
    Leave,
    /// Taken from an open pack (a joker or a planet: what the other packs' cards change isn't
    /// followed)
    Pick { key: String, name: String },
    SkipPack,
    /// At the blind select: played the blind offered, or skipped it for its tag
    PlayBlind { blind: String },
    SkipBlind { blind: String },
    /// The jokers moved to this order (names, left to right), alongside whatever else was done
    /// between the saves; only a move of jokers whose order can change a score
    /// (`advise::order_free`) counts
    Order { order: Vec<String> },
}

impl Choice {
    pub fn describe(&self) -> String {
        match self {
            Choice::Play { cards, .. } => format!("play {}", cards.join(" ")),
            Choice::Discard { cards, .. } => format!("discard {}", cards.join(" ")),
            Choice::Buy { name, sold: Some(s), .. } => format!("buy {name} (selling {s})"),
            Choice::Buy { name, .. } => format!("buy {name}"),
            Choice::Sell { name, .. } => format!("sell {name}"),
            Choice::Reroll => "reroll".into(),
            Choice::Leave => "leave the shop".into(),
            Choice::Pick { name, .. } => format!("pick {name}"),
            Choice::SkipPack => "skip the pack".into(),
            Choice::PlayBlind { blind } => format!("play {blind}"),
            Choice::SkipBlind { blind } => format!("skip {blind}"),
            Choice::Order { order } => format!("jokers ordered {}", order.join(", ")),
        }
    }

    /// A play or discard as `advise::Options::judge` names it ("action p,q,r []": places in the
    /// hand, no consumable used first)
    pub fn judge_name(&self) -> Option<String> {
        let (action, positions) = match self {
            Choice::Play { positions, .. } => ("play", positions),
            Choice::Discard { positions, .. } => ("discard", positions),
            _ => return None,
        };
        Some(format!("{action} {} []", positions.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",")))
    }
}

/// Why two saves in a row don't show one choice to compare
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unclear {
    /// A hand the hands log couldn't follow (`handlog::Skip`)
    Hand(handlog::Skip),
    /// More than one discard, or a discard and a hand, between the saves
    DiscardMissed,
    /// A consumable used (or sold) between the saves: what it was used on isn't followed
    ConsumableUsed,
    /// The jokers moved, or several things bought, sold or rerolled between the saves
    SeveralChanges,
    /// A card taken from a Tarot, Spectral or Standard pack: what it changed isn't followed
    PackCard,
    /// The save after it isn't one a single choice leads to (a screen not followed)
    Unknown,
}

impl Unclear {
    pub fn describe(self) -> String {
        match self {
            Unclear::Hand(s) => format!("a hand: {}", s.describe()),
            Unclear::DiscardMissed => "more than one discard, or a discard and a hand, between two saves".into(),
            Unclear::ConsumableUsed => "a consumable used or sold between two saves: what it changed isn't followed".into(),
            Unclear::SeveralChanges => "jokers moved, or several buys, sales or rerolls between two saves".into(),
            Unclear::PackCard => "a card taken from a Tarot, Spectral or Standard pack: what it changed isn't followed".into(),
            Unclear::Unknown => "the save after it isn't one a single choice leads to".into(),
        }
    }
}

pub enum Step {
    Chose(Choice),
    Unclear(Unclear),
    /// No choice of the owner's between them (the round's end, the cash-out, another run)
    Nothing,
}

/// The owner's choice between two saves of a run in a row, `before` with its card ids `bids`.
pub fn step(before: &RunState, bids: &CardIds, after: &RunState, aids: &CardIds, data: &GameData) -> Step {
    if after.seed != before.seed {
        return Step::Nothing;
    }
    match before.screen {
        Screen::SelectingHand => in_blind(before, bids, after, aids, data),
        Screen::Shop if before.open_pack.is_empty() => in_shop(before, after),
        s if s.in_pack() && !before.open_pack.is_empty() => in_pack(before, after, data),
        Screen::BlindSelect => at_blind_select(before, after),
        _ => Step::Nothing,
    }
}

fn in_blind(before: &RunState, bids: &CardIds, after: &RunState, aids: &CardIds, data: &GameData) -> Step {
    if before.current_blind.is_none() || after.round != before.round {
        return Step::Nothing;
    }
    if after.hands_played > before.hands_played {
        return match handlog::pair(before, bids, after, aids, data, 0) {
            Pairing::Hand(h) => {
                let Some(hand_ids) = bids.hand_ids() else { return Step::Unclear(Unclear::Hand(handlog::Skip::NoIds)) };
                let mut positions: Vec<usize> = h.ids.iter().filter_map(|id| hand_ids.iter().position(|x| x == id)).collect();
                positions.sort();
                let cards = positions.iter().map(|&p| before.hand[p].label()).collect();
                Step::Chose(Choice::Play { positions, cards })
            }
            Pairing::Skipped(s) => Step::Unclear(Unclear::Hand(s)),
            Pairing::NotAHand => Step::Nothing,
        };
    }
    if after.discards_left >= before.discards_left || !after.screen.in_blind() {
        return Step::Nothing;
    }
    if after.discards_left + 1 != before.discards_left {
        return Step::Unclear(Unclear::DiscardMissed);
    }
    if !same_names(before.consumables.iter().map(|c| &c.name), after.consumables.iter().map(|c| &c.name)) {
        return Step::Unclear(Unclear::ConsumableUsed);
    }
    if before.jokers.iter().map(|j| &j.key).ne(after.jokers.iter().map(|j| &j.key)) {
        return Step::Unclear(Unclear::Hand(handlog::Skip::JokersChanged));
    }
    let (Some(hand_ids), Some(was), Some(after_hand), Some(after_disc)) = (bids.hand_ids(), bids.discard_ids(), aids.hand_ids(), aids.discard_ids()) else {
        return Step::Unclear(Unclear::Hand(handlog::Skip::NoIds));
    };
    if hand_ids.len() != before.hand.len() {
        return Step::Unclear(Unclear::Hand(handlog::Skip::CardsUnclear));
    }
    // the cards that left the hand; every card new on the pile is one of them (one that left and
    // isn't on the pile was destroyed as it was discarded: Trading Card)
    let leaving: Vec<usize> = (0..hand_ids.len()).filter(|&i| !after_hand.contains(&hand_ids[i])).collect();
    let new_disc: Vec<i64> = after_disc.iter().copied().filter(|i| !was.contains(i)).collect();
    if leaving.is_empty() || leaving.len() > 5 || new_disc.iter().any(|i| !leaving.iter().any(|&l| hand_ids[l] == *i)) {
        return Step::Unclear(Unclear::Hand(handlog::Skip::CardsUnclear));
    }
    let cards = leaving.iter().map(|&p| before.hand[p].label()).collect();
    Step::Chose(Choice::Discard { positions: leaving, cards })
}

/// Keys counted, as many times as they appear
fn counts<'a>(keys: impl Iterator<Item = &'a String>) -> BTreeMap<&'a str, i64> {
    let mut m = BTreeMap::new();
    for k in keys {
        *m.entry(k.as_str()).or_insert(0) += 1;
    }
    m
}

/// What `after` has more of than `before` (and what it has less of), by key
fn added<'a>(before: &BTreeMap<&'a str, i64>, after: &BTreeMap<&'a str, i64>) -> Vec<&'a str> {
    let mut out = vec![];
    for (k, n) in after {
        for _ in 0..(n - before.get(k).copied().unwrap_or(0)).max(0) {
            out.push(*k);
        }
    }
    out
}

fn same_names<'a>(a: impl Iterator<Item = &'a String>, b: impl Iterator<Item = &'a String>) -> bool {
    counts(a) == counts(b)
}

fn in_shop(before: &RunState, after: &RunState) -> Step {
    let (bj, aj) = (counts(before.jokers.iter().map(|j| &j.key)), counts(after.jokers.iter().map(|j| &j.key)));
    let (joker_in, joker_out) = (added(&bj, &aj), added(&aj, &bj));
    let (bc, ac) = (counts(before.consumables.iter().map(|c| &c.key)), counts(after.consumables.iter().map(|c| &c.key)));
    let (card_in, card_out) = (added(&bc, &ac), added(&ac, &bc));
    let (bv, av) = (counts(before.vouchers.iter()), counts(after.vouchers.iter()));
    let voucher_in = added(&bv, &av);
    let jokers_moved = joker_in.is_empty() && joker_out.is_empty() && before.jokers.iter().map(|j| &j.key).ne(after.jokers.iter().map(|j| &j.key));
    if !card_out.is_empty() {
        return Step::Unclear(Unclear::ConsumableUsed);
    }
    let shop_cards = |r: &RunState| r.shop.as_ref().map(|s| (s.jokers.iter().map(|j| j.key.clone()).collect::<Vec<_>>(), s.other_cards.iter().map(|c| c.key.clone()).collect::<Vec<_>>()));
    let name = |key: &str, from: &RunState| from.shop.as_ref().and_then(|s| s.jokers.iter().find(|j| j.key == key).map(|j| j.name.clone()).or_else(|| s.other_cards.iter().chain(&s.vouchers).chain(&s.boosters).find(|c| c.key == key).map(|c| c.name.clone()))).unwrap_or_else(|| key.to_string());
    if joker_in.len() > 1 || joker_out.len() > 1 {
        return Step::Unclear(Unclear::SeveralChanges);
    }
    // a joker bought in place of one sold is one choice (the shop ranking's swap)
    let owned_changes = usize::from(!joker_in.is_empty() || !joker_out.is_empty()) + card_in.len() + voucher_in.len();
    if owned_changes > 1 {
        return Step::Unclear(Unclear::SeveralChanges);
    }
    let in_shop_after = after.screen == Screen::Shop;
    let rerolled = in_shop_after && shop_cards(before) != shop_cards(after) && after.dollars < before.dollars;
    // a pack bought opens at once (the save after it is the pack's screen)
    if after.screen.in_pack() {
        if owned_changes > 0 {
            return Step::Unclear(Unclear::SeveralChanges);
        }
        let (bb, ab) = (before.shop.as_ref().map(|s| counts(s.boosters.iter().map(|b| &b.key))).unwrap_or_default(), after.shop.as_ref().map(|s| counts(s.boosters.iter().map(|b| &b.key))).unwrap_or_default());
        let gone = added(&ab, &bb);
        let key = match gone.as_slice() {
            [k] => k.to_string(),
            _ => match before.shop.as_ref().map(|s| s.boosters.iter().filter(|b| b.key.contains(pack_kind(after.screen))).collect::<Vec<_>>()) {
                Some(v) if v.len() == 1 => v[0].key.clone(),
                _ => return Step::Unclear(Unclear::Unknown),
            },
        };
        return Step::Chose(Choice::Buy { item: "pack".into(), name: name(&key, before), key, sold: None });
    }
    if let [k] = joker_in.as_slice() {
        let sold = joker_out.first().map(|s| s.to_string());
        if rerolled && shop_cards(before).is_some_and(|(j, _)| !j.iter().any(|x| x == k)) {
            return Step::Unclear(Unclear::SeveralChanges);
        }
        return Step::Chose(Choice::Buy { item: "joker".into(), key: k.to_string(), name: name(k, before), sold });
    }
    if let [k] = joker_out.as_slice() {
        if rerolled || after.dollars <= before.dollars {
            return Step::Unclear(Unclear::SeveralChanges);
        }
        let n = before.jokers.iter().find(|j| j.key == *k).map_or(k.to_string(), |j| j.name.clone());
        return Step::Chose(Choice::Sell { key: k.to_string(), name: n });
    }
    if let [k] = voucher_in.as_slice() {
        return Step::Chose(Choice::Buy { item: "voucher".into(), key: k.to_string(), name: name(k, before), sold: None });
    }
    if let [k] = card_in.as_slice() {
        let set = before.shop.as_ref().and_then(|s| s.other_cards.iter().find(|c| c.key == *k)).map(|c| c.set.as_str()).unwrap_or("");
        let item = match set {
            "Planet" => "planet",
            "Tarot" | "Spectral" => "tarot",
            _ => "card",
        };
        return Step::Chose(Choice::Buy { item: item.into(), key: k.to_string(), name: name(k, before), sold: None });
    }
    if after.screen == Screen::BlindSelect {
        return if after.dollars == before.dollars { Step::Chose(Choice::Leave) } else { Step::Unclear(Unclear::SeveralChanges) };
    }
    if rerolled {
        return Step::Chose(Choice::Reroll);
    }
    // only the jokers moved (`reorder` reads that)
    if jokers_moved && after.screen == Screen::Shop && after.dollars == before.dollars {
        return Step::Nothing;
    }
    Step::Unclear(Unclear::Unknown)
}

/// The part of a booster's key that names its kind (`p_buffoon_normal_1`)
fn pack_kind(s: Screen) -> &'static str {
    match s {
        Screen::BuffoonPack => "buffoon",
        Screen::PlanetPack => "celestial",
        Screen::TarotPack => "arcana",
        Screen::SpectralPack => "spectral",
        Screen::StandardPack => "standard",
        _ => "?",
    }
}

fn in_pack(before: &RunState, after: &RunState, data: &GameData) -> Step {
    let (bj, aj) = (counts(before.jokers.iter().map(|j| &j.key)), counts(after.jokers.iter().map(|j| &j.key)));
    let joker_in = added(&bj, &aj);
    let pack_keys = counts(before.open_pack.iter().map(|c| &c.key));
    let name = |k: &str| before.open_pack.iter().find(|c| c.key == k).map_or(k.to_string(), |c| c.name.clone());
    match joker_in.as_slice() {
        [k] if pack_keys.contains_key(k) => return Step::Chose(Choice::Pick { key: k.to_string(), name: name(k) }),
        [] => {}
        _ => return Step::Unclear(Unclear::SeveralChanges),
    }
    // a planet picked levels its hand at once
    let levelled: Vec<&String> = after.hand_levels.iter().filter(|(h, l)| before.hand_levels.get(*h).is_some_and(|b| l.level > b.level)).map(|(h, _)| h).collect();
    if let (Screen::PlanetPack, [h]) = (before.screen, levelled.as_slice()) {
        let planet = before.open_pack.iter().find(|c| data.center(&c.key).and_then(|x| x.config.get("hand_type")).and_then(|v| v.as_str()) == Some(h.as_str()));
        if let Some(c) = planet {
            return Step::Chose(Choice::Pick { key: c.key.clone(), name: c.name.clone() });
        }
    }
    let left = !after.screen.in_pack();
    let same_deck = before.draw_pile.len() + before.hand.len() + before.discard_pile.len() == after.draw_pile.len() + after.hand.len() + after.discard_pile.len();
    let nothing_taken = levelled.is_empty() && same_deck && same_names(before.consumables.iter().map(|c| &c.key), after.consumables.iter().map(|c| &c.key)) && after.dollars == before.dollars;
    if left && nothing_taken && after.open_pack.is_empty() {
        // a Mega pack's second pick skipped looks the same: skipping what's left of it
        return Step::Chose(Choice::SkipPack);
    }
    if matches!(before.screen, Screen::TarotPack | Screen::SpectralPack | Screen::StandardPack) {
        return Step::Unclear(Unclear::PackCard);
    }
    Step::Unclear(Unclear::Unknown)
}

fn at_blind_select(before: &RunState, after: &RunState) -> Step {
    let Some(offered) = before.blinds.iter().find(|b| b.state == "Select") else { return Step::Nothing };
    if after.skips > before.skips && after.screen == Screen::BlindSelect {
        return if after.skips == before.skips + 1 { Step::Chose(Choice::SkipBlind { blind: offered.name.clone() }) } else { Step::Unclear(Unclear::SeveralChanges) };
    }
    if after.screen.in_blind() && after.current_blind.as_ref().is_some_and(|c| c.key == offered.key) {
        return Step::Chose(Choice::PlayBlind { blind: offered.name.clone() });
    }
    Step::Nothing
}

/// The jokers' new order, when the same jokers were moved between the saves so that the ones
/// whose order can change a score (`advise::order_free`; every joker beside a Blueprint or
/// Brainstorm) sit in another order
pub fn reorder(before: &RunState, after: &RunState, data: &GameData) -> Option<Choice> {
    if after.seed != before.seed || before.jokers.iter().map(|j| &j.key).eq(after.jokers.iter().map(|j| &j.key)) {
        return None;
    }
    if counts(before.jokers.iter().map(|j| &j.key)) != counts(after.jokers.iter().map(|j| &j.key)) {
        return None;
    }
    let movable = |r: &RunState| {
        let b = crate::engine::Board::from_run(r, data);
        let copiers = b.jokers.iter().any(|j| matches!(j.kind, crate::engine::Kind::Blueprint | crate::engine::Kind::Brainstorm));
        b.jokers.iter().filter(|j| copiers || !crate::advise::order_free(j)).map(|j| j.key.clone()).collect::<Vec<_>>()
    };
    (movable(before) != movable(after)).then(|| Choice::Order { order: after.jokers.iter().map(|j| data.name(&j.key).to_string()).collect() })
}

/// One run's saved states, oldest first (`history/<seed>/*.jkr`)
pub fn run_states(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "jkr")).collect()).unwrap_or_default();
    files.sort();
    files
}

/// Each choice a run's saved states show, with the state it was made from
pub type Chosen = Vec<(PathBuf, Choice)>;

/// Each choice a run's saved states show, with the state it was made from, and what couldn't
/// be read (with the state before it)
pub fn scan_run(dir: &Path, data: &GameData) -> (Chosen, Vec<(PathBuf, Unclear)>) {
    let (mut chosen, mut unclear) = (vec![], vec![]);
    let mut prev: Option<(PathBuf, RunState, CardIds)> = None;
    for f in run_states(dir) {
        let Ok((r, ids)) = save::load_with_ids(&f, data) else { continue };
        if let Some((p, b, bids)) = &prev {
            if let Some(o) = reorder(b, &r, data) {
                chosen.push((p.clone(), o));
            }
            match step(b, bids, &r, &ids, data) {
                Step::Chose(c) => chosen.push((p.clone(), c)),
                Step::Unclear(u) => unclear.push((p.clone(), u)),
                Step::Nothing => {}
            }
        }
        prev = Some((f, r, ids));
    }
    (chosen, unclear)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Card;
    use crate::save::{BlindSlot, CurrentBlind, JokerCard, Shop};

    fn state(screen: &str) -> RunState {
        serde_json::from_value(serde_json::json!({
            "seed": "ABC", "won": false, "game_version": "", "screen": screen, "stake": 1, "deck": "Red Deck",
            "ante": 1, "win_ante": 8, "blind_scaling": 1, "ante_scaling": 1.0, "round": 1, "dollars": 10.0,
            "interest_amount": 1, "interest_cap": 25, "money_per_hand": 1.0, "base_reroll_cost": 5, "skips": 0, "hands_played": 0,
            "tarots_used": 0, "starting_deck_size": 52, "most_played_hand": "", "hands_left": 4, "discards_left": 3,
            "round_hands": 4, "round_discards": 3, "hand_size": 8, "joker_slots": 5, "consumable_slots": 2, "probability_normal": 1.0,
            "jokers": [], "consumables": [], "hand": [], "draw_pile": [], "discard_pile": [], "hand_levels": {}, "blinds": [],
            "current_blind": null, "shop": null, "open_pack": [], "vouchers": [], "tags": [],
            "round_targets": {"ancient_suit": null, "castle_suit": null, "idol": null, "mail_rank": null},
            "used_jokers": [], "pool_flags": [], "banned_keys": [],
            "shop_rates": {"joker": 20.0, "tarot": 4.0, "planet": 4.0, "spectral": 0.0, "playing_card": 0.0, "slots": 2},
            "snapshot": {"path": "x", "live": false, "age_secs": null, "caveats": []}
        }))
        .unwrap()
    }

    fn joker(key: &str) -> JokerCard {
        serde_json::from_value(serde_json::json!({ "key": key, "name": key, "eternal": false, "rental": false, "debuff": false, "cost": 5, "sell_value": 2, "ability": {} })).unwrap()
    }

    fn shop(jokers: &[&str]) -> Option<Shop> {
        Some(Shop { jokers: jokers.iter().map(|k| joker(k)).collect(), other_cards: vec![], boosters: vec![], vouchers: vec![], reroll_cost: 5, free_rerolls: 0 })
    }

    fn no_ids() -> CardIds {
        CardIds { hand: vec![], discard: vec![] }
    }

    fn ids(hand: &[i64], discard: &[i64]) -> CardIds {
        CardIds { hand: hand.iter().map(|&i| Some(i)).collect(), discard: discard.iter().map(|&i| Some(i)).collect() }
    }

    fn chose(s: Step) -> Choice {
        match s {
            Step::Chose(c) => c,
            Step::Unclear(u) => panic!("unclear: {u:?}"),
            Step::Nothing => panic!("nothing"),
        }
    }

    fn in_blind(hand: &[&str], discard: &[&str], discards_left: i64) -> RunState {
        let mut r = state("selecting_hand");
        let cards = |v: &[&str]| v.iter().map(|c| Card::parse(c).unwrap()).collect();
        r.hand = cards(hand);
        r.discard_pile = cards(discard);
        r.discards_left = discards_left;
        r.current_blind = Some(CurrentBlind { key: "bl_small".into(), name: "Small Blind".into(), target: 300.0, scored: 0.0, disabled: false, hands_seen: vec![], only_hand: None });
        r
    }

    #[test]
    fn a_discard_is_the_cards_that_left_the_hand_for_the_pile_by_their_places() {
        let before = in_blind(&["KS", "2C", "KH", "7D"], &[], 3);
        let after = in_blind(&["KS", "KH", "3S", "4S"], &["2C", "7D"], 2);
        let c = chose(step(&before, &ids(&[1, 2, 3, 4], &[]), &after, &ids(&[1, 3, 5, 6], &[2, 4]), GameData::bundled()));
        assert_eq!(c, Choice::Discard { positions: vec![1, 3], cards: vec!["2♣".into(), "7♦".into()] });
        assert_eq!(c.judge_name().as_deref(), Some("discard 1,3 []"));
        // two discards between the saves: not one choice
        let after = in_blind(&["3S"], &["2C", "7D", "KS", "KH"], 1);
        assert!(matches!(step(&before, &ids(&[1, 2, 3, 4], &[]), &after, &ids(&[5], &[2, 4, 1, 3]), GameData::bundled()), Step::Unclear(Unclear::DiscardMissed)));
    }

    #[test]
    fn a_hand_played_is_named_by_its_places_for_the_judge() {
        let before = in_blind(&["2C", "KS", "7D", "KH"], &[], 3);
        let mut after = in_blind(&["2C", "7D", "3S", "4S"], &["KS", "KH"], 3);
        after.hands_played = 1;
        after.current_blind.as_mut().unwrap().scored = 60.0;
        let c = chose(step(&before, &ids(&[1, 2, 3, 4], &[]), &after, &ids(&[1, 3, 5, 6], &[2, 4]), GameData::bundled()));
        assert_eq!(c.judge_name().as_deref(), Some("play 1,3 []"));
    }

    #[test]
    fn shop_choices_from_what_changed() {
        let mut before = state("shop");
        before.jokers = vec![joker("j_egg")];
        before.shop = shop(&["j_joker", "j_popcorn"]);
        // bought: a joker more, money down
        let mut after = before.clone();
        after.jokers.push(joker("j_joker"));
        after.dollars = 5.0;
        after.shop = shop(&["j_popcorn"]);
        assert_eq!(chose(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::Buy { item: "joker".into(), key: "j_joker".into(), name: "j_joker".into(), sold: None });
        // a swap: one sold, one bought, one choice (the ranking's buy that replaces)
        after.jokers = vec![joker("j_joker")];
        assert!(matches!(chose(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::Buy { sold: Some(s), .. } if s == "j_egg"));
        // sold: a joker less, money up
        let mut after = before.clone();
        after.jokers.clear();
        after.dollars = 12.0;
        assert_eq!(chose(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::Sell { key: "j_egg".into(), name: "j_egg".into() });
        // rerolled: the shop's cards changed, money down, nothing owned changed
        let mut after = before.clone();
        after.shop = shop(&["j_cavendish", "j_ramen"]);
        after.dollars = 5.0;
        assert_eq!(chose(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::Reroll);
        // left: the blind select next, nothing else changed
        let mut after = before.clone();
        after.screen = Screen::BlindSelect;
        assert_eq!(chose(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::Leave);
        // two buys between the saves can't be told apart
        let mut after = before.clone();
        after.jokers.extend([joker("j_joker"), joker("j_popcorn")]);
        assert!(matches!(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled()), Step::Unclear(Unclear::SeveralChanges)));
        // jokers moved and one bought: the move is read apart (`reorder`), the buy as a buy
        let mut b2 = before.clone();
        b2.jokers.push(joker("j_cavendish"));
        let mut after = b2.clone();
        after.jokers.reverse();
        after.jokers.push(joker("j_joker"));
        after.dollars = 5.0;
        assert!(matches!(chose(step(&b2, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::Buy { key, .. } if key == "j_joker"));
    }

    #[test]
    fn jokers_moved_count_only_where_their_order_can_change_a_score() {
        let mut before = state("shop");
        before.jokers = vec![joker("j_joker"), joker("j_cavendish"), joker("j_banner")];
        let mut after = before.clone();
        after.jokers = vec![joker("j_cavendish"), joker("j_joker"), joker("j_banner")];
        assert!(matches!(reorder(&before, &after, GameData::bundled()), Some(Choice::Order { order }) if order[0] == "Cavendish"));
        // only the move: no other choice between the saves
        assert!(matches!(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled()), Step::Nothing));
        // Banner's chips don't care where it sits: moving it isn't a choice
        after.jokers = vec![joker("j_banner"), joker("j_joker"), joker("j_cavendish")];
        assert!(reorder(&before, &after, GameData::bundled()).is_none());
    }

    #[test]
    fn a_blind_skipped_or_played() {
        let mut before = state("blind_select");
        before.blinds = vec![BlindSlot { slot: "Small".into(), key: "bl_small".into(), name: "Small Blind".into(), state: "Select".into(), target: 300.0, reward: 3, skip_tag: Some("tag_economy".into()) }];
        let mut after = before.clone();
        after.skips = 1;
        assert_eq!(chose(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::SkipBlind { blind: "Small Blind".into() });
        let mut after = in_blind(&["KS"], &[], 3);
        after.blinds = before.blinds.clone();
        assert_eq!(chose(step(&before, &no_ids(), &after, &no_ids(), GameData::bundled())), Choice::PlayBlind { blind: "Small Blind".into() });
    }
}
