//! Poker hand detection: a port of `evaluate_poker_hand`, `get_flush`,
//! `get_straight`, `get_X_same`, `get_highest` (misc_functions.lua) and
//! `G.FUNCS.get_poker_hand_info` (state_events.lua).

use serde::{Deserialize, Serialize};

use crate::model::{Card, Enhancement, Suit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HandType {
    FlushFive,
    FlushHouse,
    FiveOfAKind,
    StraightFlush,
    FourOfAKind,
    FullHouse,
    Flush,
    Straight,
    ThreeOfAKind,
    TwoPair,
    Pair,
    HighCard,
}

impl HandType {
    /// Strongest first, the order `get_poker_hand_info` checks them.
    pub const ALL: [HandType; 12] = [
        HandType::FlushFive,
        HandType::FlushHouse,
        HandType::FiveOfAKind,
        HandType::StraightFlush,
        HandType::FourOfAKind,
        HandType::FullHouse,
        HandType::Flush,
        HandType::Straight,
        HandType::ThreeOfAKind,
        HandType::TwoPair,
        HandType::Pair,
        HandType::HighCard,
    ];

    /// The name the save and the game use (`GAME.hands` keys, joker `type` fields).
    pub fn name(self) -> &'static str {
        crate::model::POKER_HANDS[self as usize]
    }

    pub fn from_name(s: &str) -> Option<HandType> {
        crate::model::POKER_HANDS.iter().position(|n| *n == s).map(|i| HandType::ALL[i])
    }

    pub fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Jokers that change what counts as a hand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct RuleFlags {
    pub four_fingers: bool,
    pub shortcut: bool,
    pub smeared: bool,
    pub splash: bool,
    pub pareidolia: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandInfo {
    pub hand: HandType,
    /// Bitmask of every hand the play *contains* (`next(poker_hands[X])` in the game).
    pub contains: u16,
    /// Indices into the played cards, in play order.
    pub scoring: Vec<usize>,
}

impl HandInfo {
    pub fn contains(&self, h: HandType) -> bool {
        self.contains & h.bit() != 0
    }
}

/// `Card:get_id()`. Stone cards get a random negative id in the game, so they never
/// match anything; `-(100 + index)` keeps them distinct from each other.
pub fn card_id(c: &Card, index: usize) -> i32 {
    if c.enhancement == Some(Enhancement::Stone) { -(100 + index as i32) } else { i32::from(c.rank.0) }
}

/// `Card:is_suit(suit, bypass_debuff, flush_calc)`.
pub fn is_suit(c: &Card, suit: Suit, bypass_debuff: bool, flush_calc: bool, smeared: bool) -> bool {
    if flush_calc {
        if c.enhancement == Some(Enhancement::Stone) {
            return false;
        }
        if c.enhancement == Some(Enhancement::Wild) && !c.debuff {
            return true;
        }
    } else {
        if c.debuff && !bypass_debuff {
            return false;
        }
        if c.enhancement == Some(Enhancement::Stone) {
            return false;
        }
        if c.enhancement == Some(Enhancement::Wild) {
            return true;
        }
    }
    if smeared && c.suit.is_red() == suit.is_red() {
        return true;
    }
    c.suit == suit
}

/// `Card:is_face(from_boss)`.
pub fn is_face(c: &Card, index: usize, pareidolia: bool, from_boss: bool) -> bool {
    if c.debuff && !from_boss {
        return false;
    }
    let id = card_id(c, index);
    (11..=13).contains(&id) || pareidolia
}

/// `Card:get_nominal()`: rank chips + suit/face tie-breakers; Stone cards sink.
fn nominal(c: &Card) -> f64 {
    let (suit_nominal, suit_orig) = match c.suit {
        Suit::Diamonds => (0.01, 0.001),
        Suit::Clubs => (0.02, 0.002),
        Suit::Hearts => (0.03, 0.003),
        Suit::Spades => (0.04, 0.004),
    };
    let face = match c.rank.0 {
        11 => 0.1,
        12 => 0.2,
        13 => 0.3,
        14 => 0.4,
        _ => 0.0,
    };
    let mult = if c.enhancement == Some(Enhancement::Stone) { -1000.0 } else { 1.0 };
    c.rank.chips() + suit_nominal * mult + suit_orig * 0.0001 * mult + face
}

/// Cards as bitmasks (bit i = played card i). Played hands are at most a handful of
/// cards, so everything below works on `u32` masks with no allocation.
type Mask = u32;

fn bits(m: Mask) -> impl Iterator<Item = usize> {
    (0..32).filter(move |i| m & (1 << i) != 0)
}

/// `get_X_same`: for each group size, the highest-id group and how many groups there are.
struct Groups {
    first: [Mask; 6],
    second: [Mask; 6],
    count: [u8; 6],
}

fn groups(cards: &[Card]) -> Groups {
    let mut by_id = [0 as Mask; 15];
    for (i, c) in cards.iter().enumerate() {
        let id = card_id(c, i);
        if id > 0 {
            by_id[id as usize] |= 1 << i;
        }
    }
    let mut g = Groups { first: [0; 6], second: [0; 6], count: [0; 6] };
    for id in (1..15).rev() {
        let n = by_id[id].count_ones() as usize;
        if (2..=5).contains(&n) {
            match g.count[n] {
                0 => g.first[n] = by_id[id],
                1 => g.second[n] = by_id[id],
                _ => {}
            }
            g.count[n] += 1;
        }
    }
    g
}

/// `get_flush`
fn flush(cards: &[Card], f: RuleFlags) -> Option<Mask> {
    let need = if f.four_fingers { 4 } else { 5 };
    if cards.len() > 5 || cards.len() < need {
        return None;
    }
    Suit::ALL.iter().find_map(|&suit| {
        let t: Mask = cards.iter().enumerate().filter(|(_, c)| is_suit(c, suit, false, true, f.smeared)).fold(0, |m, (i, _)| m | 1 << i);
        (t.count_ones() as usize >= need).then_some(t)
    })
}

/// `get_straight`
fn straight(cards: &[Card], f: RuleFlags) -> Option<Mask> {
    let need = if f.four_fingers { 4 } else { 5 };
    if cards.len() > 5 || cards.len() < need {
        return None;
    }
    let mut ids = [0 as Mask; 15];
    for (i, c) in cards.iter().enumerate() {
        let id = card_id(c, i);
        if (2..15).contains(&id) {
            ids[id as usize] |= 1 << i;
        }
    }
    let (mut t, mut length, mut found, mut skipped) = (0 as Mask, 0, false, false);
    for j in 1..=14usize {
        let id = if j == 1 { 14 } else { j };
        if ids[id] != 0 {
            length += 1;
            skipped = false;
            t |= ids[id];
        } else if f.shortcut && !skipped && j != 14 {
            skipped = true;
        } else {
            length = 0;
            skipped = false;
            if found {
                break;
            }
            t = 0;
        }
        if length >= need {
            found = true;
        }
    }
    found.then_some(t)
}

/// `get_highest`
fn highest(cards: &[Card]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, c) in cards.iter().enumerate() {
        let n = nominal(c);
        if best.is_none_or(|(_, b)| n > b) {
            best = Some((i, n));
        }
    }
    best.map(|(i, _)| i)
}

/// Identifies the played hand. `played` is in play order (left to right).
pub fn detect(played: &[Card], f: RuleFlags) -> HandInfo {
    use HandType::*;
    let g = groups(played);
    let (p5, p4, p3, p2) = (g.count[5] > 0, g.count[4] > 0, g.count[3] > 0, g.count[2] > 0);
    let pf = flush(played, f);
    let ps = straight(played, f);
    let ph = highest(played);

    // found[h] = the cards of hand h, if the play contains it
    let mut found: [Option<Mask>; 12] = [None; 12];
    if p5 && pf.is_some() {
        found[FlushFive as usize] = Some(g.first[5]);
    }
    if p3 && p2 && pf.is_some() {
        found[FlushHouse as usize] = Some(g.first[3] | g.first[2]);
    }
    if p5 {
        found[FiveOfAKind as usize] = Some(g.first[5]);
    }
    if let (Some(fl), Some(st)) = (pf, ps) {
        found[StraightFlush as usize] = Some(fl | st);
    }
    if p4 {
        found[FourOfAKind as usize] = Some(g.first[4]);
    }
    if p3 && p2 {
        found[FullHouse as usize] = Some(g.first[3] | g.first[2]);
    }
    if let Some(fl) = pf {
        found[Flush as usize] = Some(fl);
    }
    if let Some(st) = ps {
        found[Straight as usize] = Some(st);
    }
    if p3 {
        found[ThreeOfAKind as usize] = Some(g.first[3]);
    }
    if g.count[2] == 2 || (g.count[3] == 1 && g.count[2] == 1) {
        let b = if g.count[2] == 2 { g.second[2] } else { g.first[3] };
        found[TwoPair as usize] = Some(g.first[2] | b);
    }
    if p2 {
        found[Pair as usize] = Some(g.first[2]);
    }
    if let Some(h) = ph {
        found[HighCard as usize] = Some(1 << h);
    }
    // "Contains" propagation at the end of evaluate_poker_hand: 5oak → 4oak → 3oak → pair.
    // The game fills these with group lists; only non-emptiness matters for `contains`.
    if found[FiveOfAKind as usize].is_some() && found[FourOfAKind as usize].is_none() {
        found[FourOfAKind as usize] = Some(0);
    }
    if found[FourOfAKind as usize].is_some() && found[ThreeOfAKind as usize].is_none() {
        found[ThreeOfAKind as usize] = Some(0);
    }
    if found[ThreeOfAKind as usize].is_some() && found[Pair as usize].is_none() {
        found[Pair as usize] = Some(0);
    }

    let contains = HandType::ALL.iter().filter(|h| found[**h as usize].is_some()).fold(0, |m, h| m | h.bit());
    let hand = HandType::ALL.into_iter().find(|h| found[*h as usize].is_some()).unwrap_or(HighCard);
    let mut scoring = found[hand as usize].unwrap_or(0);

    // evaluate_play: Splash makes every played card score; Stone cards always score.
    if f.splash {
        scoring = if played.len() >= 32 { Mask::MAX } else { (1 << played.len()) - 1 };
    } else {
        for (i, c) in played.iter().enumerate() {
            if c.enhancement == Some(Enhancement::Stone) {
                scoring |= 1 << i;
            }
        }
    }
    // table.sort(scoring_hand, by screen x) → play order, which is bit order
    HandInfo { hand, contains, scoring: bits(scoring).filter(|&i| i < played.len()).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rank;

    fn cards(spec: &str) -> Vec<Card> {
        // "AS KH 10D 2C" ; suffix * = Wild, # = Stone
        spec.split_whitespace()
            .map(|t| {
                let (body, enh) = match t.chars().last().unwrap() {
                    '*' => (&t[..t.len() - 1], Some(Enhancement::Wild)),
                    '#' => (&t[..t.len() - 1], Some(Enhancement::Stone)),
                    _ => (t, None),
                };
                let (r, s) = body.split_at(body.len() - 1);
                let rank = match r {
                    "A" => 14,
                    "K" => 13,
                    "Q" => 12,
                    "J" => 11,
                    n => n.parse().unwrap(),
                };
                let suit = match s {
                    "S" => Suit::Spades,
                    "H" => Suit::Hearts,
                    "C" => Suit::Clubs,
                    _ => Suit::Diamonds,
                };
                let mut c = Card::new(Rank(rank), suit);
                c.enhancement = enh;
                c
            })
            .collect()
    }

    fn d(spec: &str, f: RuleFlags) -> HandInfo {
        detect(&cards(spec), f)
    }

    const NONE: RuleFlags =
        RuleFlags { four_fingers: false, shortcut: false, smeared: false, splash: false, pareidolia: false };

    #[test]
    fn basic_hands() {
        use HandType::*;
        assert_eq!(d("AS", NONE).hand, HighCard);
        let p = d("KS KH 3D", NONE);
        assert_eq!((p.hand, p.scoring.clone()), (Pair, vec![0, 1]));
        assert_eq!(d("KS KH 3D 3C", NONE).hand, TwoPair);
        assert_eq!(d("KS KH KD 3C 3D", NONE).hand, FullHouse);
        assert_eq!(d("2S 3H 4D 5C 6D", NONE).hand, Straight);
        assert_eq!(d("AS 2H 3D 4C 5D", NONE).hand, Straight);
        assert_eq!(d("QS KH AD 2C 3D", NONE).hand, HighCard); // no wrap-around
        assert_eq!(d("2H 7H 9H JH KH", NONE).hand, Flush);
        assert_eq!(d("9H 10H JH QH KH", NONE).hand, StraightFlush);
        assert_eq!(d("KS KH KD KC 3D", NONE).hand, FourOfAKind);
        assert_eq!(d("KS KH KD KC KD", NONE).hand, FiveOfAKind);
        assert_eq!(d("KH KH KH 3H 3H", NONE).hand, FlushHouse);
        assert_eq!(d("KH KH KH KH KH", NONE).hand, FlushFive);
    }

    #[test]
    fn contains_semantics() {
        use HandType::*;
        let fh = d("KS KH KD 3C 3D", NONE);
        assert!(fh.contains(ThreeOfAKind) && fh.contains(Pair) && fh.contains(TwoPair));
        let quads = d("KS KH KD KC 3D", NONE);
        assert!(quads.contains(ThreeOfAKind) && quads.contains(Pair));
        assert!(!quads.contains(TwoPair)); // the game's rule: 4oak is not two pair
        let trips = d("KS KH KD", NONE);
        assert!(trips.contains(Pair));
        assert_eq!(trips.scoring, vec![0, 1, 2]);
    }

    #[test]
    fn four_fingers_and_shortcut() {
        use HandType::*;
        let ff = RuleFlags { four_fingers: true, ..NONE };
        assert_eq!(d("2H 7H 9H JH 3S", ff).hand, Flush);
        assert_eq!(d("2H 7H 9H JH 3S", ff).scoring, vec![0, 1, 2, 3]);
        assert_eq!(d("5S 6H 7D 8C KD", ff).hand, Straight);
        assert_eq!(d("5S 6H 7D 8C KD", ff).scoring, vec![0, 1, 2, 3]);
        let sc = RuleFlags { shortcut: true, ..NONE };
        assert_eq!(d("2S 4H 6D 8C 10D", sc).hand, Straight);
        assert_eq!(d("2S 4H 6D 8C 10D", NONE).hand, HighCard);
        // Two gaps in a row still break it
        assert_eq!(d("2S 3H 6D 7C 8D", sc).hand, HighCard);
    }

    #[test]
    fn wild_stone_smeared_splash() {
        use HandType::*;
        assert_eq!(d("2H 7H 9S* JH KH", NONE).hand, Flush);
        // Stone never counts for a suit or rank but always scores
        let s = d("KS KH 5D#", NONE);
        assert_eq!((s.hand, s.scoring.clone()), (Pair, vec![0, 1, 2]));
        assert_eq!(d("5D# 5D#", NONE).hand, HighCard);
        let sm = RuleFlags { smeared: true, ..NONE };
        assert_eq!(d("2H 7D 9H JD KH", sm).hand, Flush);
        let sp = RuleFlags { splash: true, ..NONE };
        assert_eq!(d("KS KH 3D 4C", sp).scoring, vec![0, 1, 2, 3]);
    }

    #[test]
    fn high_card_tiebreak() {
        // Same rank: Spades beat Hearts beat Clubs beat Diamonds (suit_nominal)
        assert_eq!(d("KD KS", NONE).hand, HandType::Pair);
        assert_eq!(d("QD JS", NONE).scoring, vec![0]);
        assert_eq!(d("10D 10S 9C", NONE).hand, HandType::Pair);
        assert_eq!(d("AD KS", NONE).scoring, vec![0]);
        // A stone sinks below everything
        assert_eq!(d("AS# 2D", NONE).hand, HandType::HighCard);
        assert!(d("AS# 2D", NONE).scoring.contains(&1));
    }
}
