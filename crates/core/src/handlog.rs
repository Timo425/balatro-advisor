//! Real hands next to the engine: a hand the owner played, found from the save before it and
//! the save after it, scored on the engine from the board before it, beside the score the game
//! gave it.
//!
//! The cards are followed by their ids (`save::CardIds`): the ones that left the hand and
//! reached the discard pile were played, or taken by The Hook (blind.lua `Blind:press_play`
//! discards them before the hand scores, so they reach the pile first); a played card missing
//! from both was destroyed while it scored (a Glass card that shattered). The played cards
//! reach the pile in the order they were played (state_events.lua
//! `G.FUNCS.draw_from_play_to_discard`). The game's score is the round's chips after the hand
//! less the chips before it.
//!
//! The live page logs each hand to `hands.jsonl` (next to `calibration.jsonl`; a hand that
//! matched without its board); `scan` pairs the saved history the same way. `balatro-advisor hands` lists what didn't match and
//! captures one as a golden case.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::data::GameData;
use crate::engine::{self, Board, Lucky, Pick, Rolls, Unlucky};
use crate::model::Card;
use crate::save::{self, CardIds, RunState, Screen};

/// One hand played, with what the engine and the game made of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hand {
    /// When it was logged (seconds), or the time in the name of the save after it
    pub time: u64,
    pub seed: String,
    pub ante: i64,
    pub round: i64,
    pub blind: String,
    /// The hand type the engine saw
    pub hand: String,
    /// The board before the hand; left out of the log for a hand that matched (`Logger`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<Board>,
    /// In the order played
    pub played: Vec<Card>,
    /// In the order they sat in the hand when it was saved, The Hook's included
    pub held: Vec<Card>,
    /// The cards The Hook took (indices into `held`)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooked: Vec<usize>,
    /// The engine's score with every roll failing, and with every roll hitting
    pub floor: f64,
    pub ceiling: f64,
    pub game: Game,
    /// What the saves leave unsure (an order guessed, a joker gone after the hand)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// The played cards' ids (`save::CardIds`), in the order played; not logged
    #[serde(skip)]
    pub ids: Vec<i64>,
}

/// The game's score for the hand.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Game {
    /// The round's chips moved by this much
    Scored(f64),
    /// The hand won the blind and the save after it no longer shows the chips: at least what
    /// the blind still needed
    AtLeast(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// No roll involved and the scores agree
    Match,
    /// A roll was involved (Lucky card, Misprint, Bloodstone…) and the game's score lies between
    /// the engine's floor and ceiling: not a mismatch, not a confirmation either
    InRange,
    /// The hand won the blind, and the engine's ceiling reaches what it needed
    Enough,
    Mismatch,
}

impl Hand {
    /// A hand played from `before` (its board `board`), scored on the engine: the floor with
    /// every roll failing, the ceiling with every roll hitting, The Hook's pick as given
    #[allow(clippy::too_many_arguments)]
    pub fn new(before: &RunState, board: Board, played: Vec<Card>, held: Vec<Card>, hooked: Vec<usize>, game: Game, notes: Vec<String>, time: u64) -> Hand {
        let mut h = Hand {
            time,
            seed: before.seed.clone(),
            ante: before.ante,
            round: before.round,
            blind: before.current_blind.as_ref().map(|b| b.name.clone()).unwrap_or_default(),
            hand: String::new(),
            board: Some(board),
            played,
            held,
            hooked,
            floor: 0.0,
            ceiling: 0.0,
            game,
            notes,
            ids: vec![],
        };
        let mut rolls = Known { lucky: false, hooked: h.hooked.clone() };
        let o = engine::score_as_played(h.board.as_ref().expect("just set"), &h.played, &h.held, &mut rolls, false);
        h.hand = o.hand.name().to_string();
        h.floor = o.score;
        h.ceiling = h.rescore(true).unwrap_or(h.floor);
        h
    }

    /// The engine's score with the rolls all failing (`lucky` false) or all hitting, and The
    /// Hook's pick as it was
    pub fn rescore(&self, lucky: bool) -> Option<f64> {
        let mut rolls = Known { lucky, hooked: self.hooked.clone() };
        Some(engine::score_as_played(self.board.as_ref()?, &self.played, &self.held, &mut rolls, false).score)
    }

    pub fn verdict(&self) -> Verdict {
        match self.game {
            Game::Scored(x) if same(x, self.floor) && same(x, self.ceiling) => Verdict::Match,
            Game::Scored(x) if self.floor < self.ceiling && x >= self.floor - tol(x) && x <= self.ceiling + tol(x) => Verdict::InRange,
            Game::AtLeast(x) if self.ceiling >= x - tol(x) => Verdict::Enough,
            _ => Verdict::Mismatch,
        }
    }

    /// Why a mismatch may have happened, as far as the hand shows: the first that applies of a
    /// score off by one chip (the game floors its chips × Mult, and a ×Mult it moves in small
    /// steps, such as Ramen's 2 − 0.01 per card, drifts in floating point where the save writes
    /// it rounded, so its 1.8199999999999998 is read as 1.82), a roll (the score outside even the
    /// range), a joker gone between the saves, a boss blind, the jokers, the cards' enhancements,
    /// editions and seals, else nothing special
    pub fn cause(&self) -> String {
        let cards = || self.played.iter().chain(&self.held).filter(|c| c.enhancement.is_some() || c.edition.is_some() || c.seal.is_some());
        if matches!(self.game, Game::Scored(x) if (x - self.floor).abs() <= 1.0) {
            "off by one chip: the final floor of a float the save rounds".into()
        } else if self.floor < self.ceiling {
            "random effects (outside the range)".into()
        } else if self.notes.iter().any(|n| n.starts_with("gone after the hand")) {
            "a joker gone between the saves".into()
        } else if self.board.as_ref().is_some_and(|b| !b.blind.disabled && !matches!(b.blind.key.as_str(), "" | "bl_small" | "bl_big")) {
            format!("boss blind ({})", self.blind)
        } else if self.board.as_ref().is_some_and(|b| !b.jokers.is_empty()) {
            "jokers".into()
        } else if cards().next().is_some() {
            "enhancements, editions, seals".into()
        } else {
            "plain cards".into()
        }
    }

    pub fn summary(&self) -> String {
        let game = match self.game {
            Game::Scored(x) => format!("{x}"),
            Game::AtLeast(x) => format!("≥ {x}"),
        };
        let engine = if self.floor == self.ceiling { format!("{}", self.floor) } else { format!("{}–{}", self.floor, self.ceiling) };
        let labels = |v: &[Card]| v.iter().map(Card::label).collect::<Vec<_>>().join(" ");
        let jokers = self.board.iter().flat_map(|b| &b.jokers).map(|j| j.key.trim_start_matches("j_").to_string()).collect::<Vec<_>>().join(",");
        format!(
            "{} a{} r{} {}: {} [{}] held [{}]{} · jokers {} · engine {engine}, game {game}",
            self.seed,
            self.ante,
            self.round,
            self.blind,
            self.hand,
            labels(&self.played),
            labels(&self.held),
            if self.hooked.is_empty() { String::new() } else { format!(" (Hook took {:?})", self.hooked) },
            if jokers.is_empty() { "none".into() } else { jokers },
        )
    }

    /// The hand as a golden case (`golden::Case`); none without its board
    pub fn golden(&self, name: &str) -> Option<crate::golden::Case> {
        Some(crate::golden::Case {
            name: name.to_string(),
            board: self.board.clone()?,
            played: self.played.clone(),
            held: self.held.clone(),
            hooked: self.hooked.clone(),
            predicted: self.floor,
            expected: match self.game {
                Game::Scored(x) => Some(x),
                Game::AtLeast(_) => None,
            },
            pending: None,
            note: format!("{} ante {} round {} {}, from the hands log", self.seed, self.ante, self.round, self.blind),
        })
    }
}

/// Scores agree (the game's numbers are Lua doubles, summed in its own order)
fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= tol(a)
}

fn tol(x: f64) -> f64 {
    x.abs() * 1e-9
}

/// Rolls all failing or all hitting, The Hook's pick given
struct Known {
    lucky: bool,
    hooked: Vec<usize>,
}

impl Rolls for Known {
    fn chance(&mut self, p: f64) -> bool {
        if self.lucky { Lucky.chance(p) } else { Unlucky.chance(p) }
    }
    fn range(&mut self, min: i64, max: i64) -> i64 {
        if self.lucky { Lucky.range(min, max) } else { Unlucky.range(min, max) }
    }
    fn unit(&mut self) -> f64 {
        0.0
    }
    fn pick(&mut self, _n: usize, _k: usize) -> Pick {
        Pick::These(self.hooked.clone())
    }
}

/// Why two saves in a row don't give a hand to compare
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// More than one hand was played between them (states not saved in between)
    Missed,
    /// A discard too (its save missed)
    DiscardToo,
    /// A consumable was used or sold before the hand (the save before it predates that)
    ConsumableGone,
    /// Jokers added or moved before the hand
    JokersChanged,
    /// The save after the hand no longer has its cards (they went back to the deck)
    CardsGone,
    /// Cards saved without ids (older saves)
    NoIds,
    /// Cards that don't add up: one reached the pile from outside the hand, none left it, or
    /// more than five were played
    CardsUnclear,
}

impl Skip {
    pub fn describe(self) -> &'static str {
        match self {
            Skip::Missed => "more than one hand between two saves (states not saved in between)",
            Skip::DiscardToo => "a discard between the saves as well",
            Skip::ConsumableGone => "a consumable used or sold before the hand",
            Skip::JokersChanged => "jokers added or moved before the hand",
            Skip::CardsGone => "the save after the hand has no cards left (round over): the cards played unknown",
            Skip::NoIds => "cards saved without ids",
            Skip::CardsUnclear => "cards that don't add up (one reached the pile from outside the hand, none left it, more than five)",
        }
    }
}

pub enum Pairing {
    Hand(Box<Hand>),
    Skipped(Skip),
    /// `after` isn't the state a hand played from `before` leads to
    NotAHand,
}

/// The hand played between two saves of a run, if `after` is the save a hand from `before` led to.
pub fn pair(before: &RunState, bids: &CardIds, after: &RunState, aids: &CardIds, data: &GameData, time: u64) -> Pairing {
    let Some(cb) = before.current_blind.as_ref().filter(|_| before.screen == Screen::SelectingHand) else { return Pairing::NotAHand };
    if after.seed != before.seed || after.round != before.round || after.hands_played <= before.hands_played {
        return Pairing::NotAHand;
    }
    if after.hands_played > before.hands_played + 1 {
        return Pairing::Skipped(Skip::Missed);
    }
    let blind_after = after.current_blind.as_ref().filter(|b| b.key == cb.key && !b.name.is_empty());
    if blind_after.is_some() && after.discards_left != before.discards_left {
        return Pairing::Skipped(Skip::DiscardToo);
    }
    let mut gone = before.consumables.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    for c in &after.consumables {
        if let Some(i) = gone.iter().position(|g| *g == c.name) {
            gone.remove(i);
        }
    }
    if !gone.is_empty() {
        return Pairing::Skipped(Skip::ConsumableGone);
    }
    // the jokers after, in their order, must be the ones before with some gone (Ice Cream
    // melting, Gros Michel at the round's end: or sold before the hand, noted)
    let mut rest = before.jokers.iter().map(|j| j.key.as_str());
    if !after.jokers.iter().all(|a| rest.by_ref().any(|b| b == a.key)) {
        return Pairing::Skipped(Skip::JokersChanged);
    }
    if after.hand.is_empty() && after.discard_pile.is_empty() {
        return Pairing::Skipped(Skip::CardsGone);
    }
    let (Some(hand_ids), Some(was_discarded), Some(after_hand), Some(after_disc)) = (bids.hand_ids(), bids.discard_ids(), aids.hand_ids(), aids.discard_ids()) else {
        return Pairing::Skipped(Skip::NoIds);
    };
    if hand_ids.len() != before.hand.len() || after_disc.len() != after.discard_pile.len() {
        return Pairing::Skipped(Skip::CardsUnclear);
    }
    let leaving: Vec<i64> = hand_ids.iter().copied().filter(|i| !after_hand.contains(i)).collect();
    let new_disc: Vec<i64> = after_disc.iter().copied().filter(|i| !was_discarded.contains(i)).collect();
    if leaving.is_empty() || new_disc.iter().any(|i| !leaving.contains(i)) {
        return Pairing::Skipped(Skip::CardsUnclear);
    }
    let board = Board::from_run(before, data);
    // The Hook's cards: as many as it takes from what the play leaves held, the first to the pile
    let k = (0..=2usize.min(leaving.len() - 1)).rev().find(|&k| engine::hook_takes(&board, hand_ids.len() - (leaving.len() - k)) == k).unwrap_or(0);
    if new_disc.len() < k {
        return Pairing::Skipped(Skip::CardsUnclear);
    }
    let hooked_ids = &new_disc[..k];
    let survivors = &new_disc[k..];
    let played_ids: Vec<i64> = leaving.iter().copied().filter(|i| !hooked_ids.contains(i)).collect();
    if played_ids.is_empty() || played_ids.len() > 5 {
        return Pairing::Skipped(Skip::CardsUnclear);
    }
    let mut notes = Vec::new();
    // the order played: the pile's, a destroyed card put where it sat in the hand
    let in_hand_order: Vec<i64> = played_ids.iter().copied().filter(|i| survivors.contains(i)).collect();
    let moved = in_hand_order != survivors;
    if moved {
        notes.push("played in another order than the hand was saved in: the cards held may have moved too".into());
    }
    let mut order: Vec<i64> = survivors.to_vec();
    for (n, &d) in played_ids.iter().enumerate().filter(|(_, i)| !survivors.contains(i)) {
        let after_this = played_ids[..n].iter().rev().find_map(|p| order.iter().position(|o| o == p));
        order.insert(after_this.map_or(0, |p| p + 1), d);
        if moved {
            notes.push("a card destroyed while it scored: its place in the play guessed".into());
        }
    }
    let card = |id: i64| before.hand[hand_ids.iter().position(|&h| h == id).expect("id from the hand")];
    let played: Vec<Card> = order.iter().map(|&i| card(i)).collect();
    let held_ids: Vec<i64> = hand_ids.iter().copied().filter(|i| !played_ids.contains(i)).collect();
    let held: Vec<Card> = held_ids.iter().map(|&i| card(i)).collect();
    let hooked: Vec<usize> = held_ids.iter().enumerate().filter(|(_, i)| hooked_ids.contains(i)).map(|(n, _)| n).collect();
    let mut kept = after.jokers.iter().map(|j| j.key.as_str()).peekable();
    for j in &before.jokers {
        if kept.peek() == Some(&j.key.as_str()) {
            kept.next();
        } else {
            notes.push(format!("gone after the hand: {} (destroyed by it, or sold before it)", j.name));
        }
    }
    let game = match blind_after {
        Some(a) => Game::Scored(a.scored - cb.scored),
        None => Game::AtLeast(cb.target - cb.scored),
    };
    let mut h = Hand::new(before, board, played, held, hooked, game, notes, time);
    h.ids = order;
    Pairing::Hand(Box::new(h))
}

/// The hands the saved history (`history/<seed>/`, oldest first) shows, and why the rest of
/// its hands don't count
#[derive(Debug, Default, Serialize)]
pub struct Scan {
    pub hands: Vec<Hand>,
    pub skipped: Vec<(Skip, usize)>,
    /// Saves read
    pub states: usize,
    /// Hands the runs played in all (the counts the saves keep, `hands_played`)
    pub hands_played: i64,
}

pub fn scan(history: &Path, data: &GameData) -> Scan {
    let mut out = Scan::default();
    let mut runs: Vec<PathBuf> = std::fs::read_dir(history).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    runs.sort();
    let mut skipped = std::collections::BTreeMap::new();
    for run in runs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&run).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "jkr")).collect()).unwrap_or_default();
        files.sort();
        let mut prev: Option<(RunState, CardIds)> = None;
        let (mut first, mut last) = (None, 0);
        for f in files {
            let Ok((r, ids)) = save::load_with_ids(&f, data) else { continue };
            out.states += 1;
            first.get_or_insert(r.hands_played);
            last = last.max(r.hands_played);
            let time = f.file_name().and_then(|n| n.to_str()).and_then(|n| n.split('-').next()).and_then(|t| t.parse::<u64>().ok()).map_or(0, |ms| ms / 1000);
            if let Some((b, bids)) = &prev {
                match pair(b, bids, &r, &ids, data, time) {
                    Pairing::Hand(h) => out.hands.push(*h),
                    Pairing::Skipped(s) => *skipped.entry(s).or_insert(0) += 1,
                    Pairing::NotAHand => {}
                }
            }
            prev = Some((r, ids));
        }
        out.hands_played += last - first.unwrap_or(last);
    }
    out.skipped = skipped.into_iter().collect();
    out
}

/// The live page's log of each hand played (`hands.jsonl`)
pub fn log_path(dir: &Path) -> PathBuf {
    dir.join("hands.jsonl")
}

pub fn read_log(dir: &Path) -> Vec<Hand> {
    std::fs::read_to_string(log_path(dir)).map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default()
}

/// Follows the states the live page sees and logs each hand played between two of them.
#[derive(Default)]
pub struct Logger {
    prev: Option<(RunState, CardIds)>,
}

impl Logger {
    /// Feed the saves in the order the game wrote them (each a copy that stays as it is). Returns
    /// the hand logged, if any.
    pub fn observe(&mut self, save: &Path, dir: &Path, data: &GameData) -> Option<Hand> {
        let Ok((r, ids)) = save::load_with_ids(save, data) else { return None };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let hand = match self.prev.take() {
            Some((b, bids)) => match pair(&b, &bids, &r, &ids, data, now) {
                Pairing::Hand(h) => Some(*h),
                _ => None,
            },
            None => None,
        };
        self.prev = Some((r, ids));
        let h = hand?;
        // a hand that matched is logged without its board (a few KB a hand): only the others are
        // ever captured
        let line = if h.verdict() == Verdict::Match { Hand { board: None, ..h.clone() } } else { h.clone() };
        if let (Ok(()), Ok(mut f)) = (std::fs::create_dir_all(dir), std::fs::OpenOptions::new().create(true).append(true).open(log_path(dir))) {
            let _ = writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default());
        }
        Some(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::CurrentBlind;

    fn state(blind: &str, hand: &[&str], discard: &[&str], scored: f64, hands_played: i64) -> RunState {
        let mut r: RunState = serde_json::from_value(serde_json::json!({
            "seed": "ABC", "won": false, "game_version": "", "screen": "selecting_hand", "stake": 1, "deck": "Red Deck",
            "ante": 1, "win_ante": 8, "blind_scaling": 1, "ante_scaling": 1.0, "round": 1, "dollars": 4.0,
            "interest_amount": 1, "interest_cap": 25, "money_per_hand": 1.0, "base_reroll_cost": 5, "skips": 0, "hands_played": hands_played,
            "tarots_used": 0, "starting_deck_size": 52, "most_played_hand": "", "hands_left": 4 - hands_played, "discards_left": 3,
            "round_hands": 4, "round_discards": 3, "hand_size": 8, "joker_slots": 5, "consumable_slots": 2, "probability_normal": 1.0,
            "jokers": [], "consumables": [], "hand": [], "draw_pile": [], "discard_pile": [], "hand_levels": {}, "blinds": [],
            "current_blind": null, "shop": null, "open_pack": [], "vouchers": [], "tags": [],
            "round_targets": {"ancient_suit": null, "castle_suit": null, "idol": null, "mail_rank": null},
            "used_jokers": [], "pool_flags": [], "banned_keys": [],
            "shop_rates": {"joker": 20.0, "tarot": 4.0, "planet": 4.0, "spectral": 0.0, "playing_card": 0.0, "slots": 2},
            "snapshot": {"path": "x", "live": false, "age_secs": null, "caveats": []}
        }))
        .unwrap();
        let cards = |v: &[&str]| v.iter().map(|c| Card::parse(c).unwrap()).collect();
        r.hand = cards(hand);
        r.discard_pile = cards(discard);
        r.current_blind = Some(CurrentBlind { key: blind.into(), name: blind.into(), target: 300.0, scored, disabled: false, hands_seen: vec![], only_hand: None });
        r
    }

    fn ids(hand: &[i64], discard: &[i64]) -> CardIds {
        CardIds { hand: hand.iter().map(|&i| Some(i)).collect(), discard: discard.iter().map(|&i| Some(i)).collect() }
    }

    #[test]
    fn a_hand_is_the_cards_that_left_for_the_pile_in_the_order_played() {
        // KS KH played from KS 2C KH 7D: the pile shows them in play order, two cards drawn
        let before = state("bl_small", &["KS", "2C", "KH", "7D"], &[], 0.0, 0);
        let after = state("bl_small", &["2C", "7D", "3S", "4S"], &["KS", "KH"], 60.0, 1);
        let Pairing::Hand(h) = pair(&before, &ids(&[1, 2, 3, 4], &[]), &after, &ids(&[2, 4, 5, 6], &[1, 3]), GameData::bundled(), 0) else { panic!("a hand") };
        assert_eq!(h.hand, "Pair");
        assert_eq!(h.played.iter().map(Card::label).collect::<Vec<_>>(), ["K♠", "K♥"]);
        assert_eq!(h.held.iter().map(Card::label).collect::<Vec<_>>(), ["2♣", "7♦"]);
        // Pair L1: (10 + 10 + 10) × 2
        assert_eq!((h.floor, h.game, h.verdict()), (60.0, Game::Scored(60.0), Verdict::Match));
        let off = Hand { game: Game::Scored(70.0), ..*h };
        assert_eq!(off.verdict(), Verdict::Mismatch);
    }

    #[test]
    fn the_hooks_cards_reach_the_pile_first() {
        // blind.lua `Blind:press_play`: The Hook discards 2 held cards before the hand scores
        let before = state("bl_hook", &["KS", "2C", "KH", "7D", "9H"], &[], 0.0, 0);
        let after = state("bl_hook", &["9H", "3S", "4S", "5S", "6S"], &["7D", "2C", "KS", "KH"], 60.0, 1);
        let Pairing::Hand(h) = pair(&before, &ids(&[1, 2, 3, 4, 5], &[]), &after, &ids(&[5, 6, 7, 8, 9], &[4, 2, 1, 3]), GameData::bundled(), 0) else { panic!("a hand") };
        assert_eq!(h.played.iter().map(Card::label).collect::<Vec<_>>(), ["K♠", "K♥"]);
        assert_eq!(h.held.iter().map(Card::label).collect::<Vec<_>>(), ["2♣", "7♦", "9♥"]);
        assert_eq!(h.hooked, [0, 1]);
        assert_eq!(h.verdict(), Verdict::Match);
    }

    #[test]
    fn a_discard_or_a_second_hand_between_the_saves_is_skipped() {
        let before = state("bl_small", &["KS", "KH"], &[], 0.0, 0);
        let mut after = state("bl_small", &["3S"], &["KS", "KH"], 60.0, 1);
        after.discards_left = 2;
        let p = pair(&before, &ids(&[1, 2], &[]), &after, &ids(&[3], &[1, 2]), GameData::bundled(), 0);
        assert!(matches!(p, Pairing::Skipped(Skip::DiscardToo)));
        let after = state("bl_small", &["3S"], &["KS", "KH"], 60.0, 2);
        let p = pair(&before, &ids(&[1, 2], &[]), &after, &ids(&[3], &[1, 2]), GameData::bundled(), 0);
        assert!(matches!(p, Pairing::Skipped(Skip::Missed)));
    }
}
