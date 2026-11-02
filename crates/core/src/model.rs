//! Plain data types shared by the save reader and (later) the scoring engine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Suit {
    Spades,
    Hearts,
    Clubs,
    Diamonds,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Spades, Suit::Hearts, Suit::Clubs, Suit::Diamonds];

    pub fn from_name(s: &str) -> Option<Suit> {
        Some(match s {
            "Spades" => Suit::Spades,
            "Hearts" => Suit::Hearts,
            "Clubs" => Suit::Clubs,
            "Diamonds" => Suit::Diamonds,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Suit::Spades => "Spades",
            Suit::Hearts => "Hearts",
            Suit::Clubs => "Clubs",
            Suit::Diamonds => "Diamonds",
        }
    }

    pub fn is_red(self) -> bool {
        matches!(self, Suit::Hearts | Suit::Diamonds)
    }
}

/// Rank id as the game uses it: 2..=10, Jack 11, Queen 12, King 13, Ace 14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rank(pub u8);

impl Rank {
    pub const JACK: Rank = Rank(11);
    pub const QUEEN: Rank = Rank(12);
    pub const KING: Rank = Rank(13);
    pub const ACE: Rank = Rank(14);

    pub fn from_value_name(s: &str) -> Option<Rank> {
        Some(Rank(match s {
            "Jack" => 11,
            "Queen" => 12,
            "King" => 13,
            "Ace" => 14,
            n => n.parse().ok().filter(|n| (2..=10).contains(n))?,
        }))
    }

    /// Chips a scored card of this rank gives (`base.nominal` in the game).
    pub fn chips(self) -> f64 {
        match self.0 {
            14 => 11.0,
            11..=13 => 10.0,
            n => f64::from(n),
        }
    }

    pub fn is_face(self) -> bool {
        (11..=13).contains(&self.0)
    }

    pub fn short(self) -> &'static str {
        ["?", "?", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"][self.0 as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Enhancement {
    Bonus,
    Mult,
    Wild,
    Glass,
    Steel,
    Stone,
    Gold,
    Lucky,
}

impl Enhancement {
    /// From a center key (`m_glass`); `c_base` and unknown keys → `None`.
    pub fn from_key(k: &str) -> Option<Enhancement> {
        Some(match k {
            "m_bonus" => Enhancement::Bonus,
            "m_mult" => Enhancement::Mult,
            "m_wild" => Enhancement::Wild,
            "m_glass" => Enhancement::Glass,
            "m_steel" => Enhancement::Steel,
            "m_stone" => Enhancement::Stone,
            "m_gold" => Enhancement::Gold,
            "m_lucky" => Enhancement::Lucky,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Foil,
    Holo,
    Polychrome,
    Negative,
}

impl Edition {
    /// From the save's `edition.type` (`foil`, `holo`, `polychrome`, `negative`).
    pub fn from_type(s: &str) -> Option<Edition> {
        Some(match s {
            "foil" => Edition::Foil,
            "holo" => Edition::Holo,
            "polychrome" => Edition::Polychrome,
            "negative" => Edition::Negative,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Seal {
    Red,
    Blue,
    Gold,
    Purple,
}

impl Seal {
    pub fn from_name(s: &str) -> Option<Seal> {
        Some(match s {
            "Red" => Seal::Red,
            "Blue" => Seal::Blue,
            "Gold" => Seal::Gold,
            "Purple" => Seal::Purple,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enhancement: Option<Enhancement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seal: Option<Seal>,
    /// Permanent extra chips (Hiker, etc.).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub perma_bonus: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub debuff: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub face_down: bool,
    /// Played this ante (`ability.played_this_ante`: state_events.lua
    /// `G.FUNCS.play_cards_from_highlighted` sets it on each card played, `end_round` clears it on
    /// every card once a boss is beaten); The Pillar debuffs it (`Board::at_blind_set`)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub played_this_ante: bool,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Card {
        Card { rank, suit, enhancement: None, edition: None, seal: None, perma_bonus: 0.0, debuff: false, face_down: false, played_this_ante: false }
    }

    /// The same card as far as play goes (rank, suit, enhancement, seal, edition), whatever its
    /// debuff or face-down state.
    pub fn same_kind(&self, other: &Card) -> bool {
        self.rank == other.rank && self.suit == other.suit && self.enhancement == other.enhancement && self.seal == other.seal && self.edition == other.edition
    }

    /// A fixed order over every field (rank high first, then suit, then the rest): whatever
    /// order cards come in, sorting by this gives the same result, so the advice never
    /// depends on how a hand or pile happened to be ordered.
    pub fn order_key(&self) -> (std::cmp::Reverse<u8>, u8, String) {
        (
            std::cmp::Reverse(self.rank.0),
            self.suit as u8,
            // a card played this ante marked only on it: every other card's key as it was (The Hook's
            // pick hashes it, `sim` `boss_takes_held`)
            format!("{:?}|{:?}|{:?}|{}|{}|{}{}", self.enhancement, self.seal, self.edition, self.perma_bonus, self.debuff, self.face_down, if self.played_this_ante { "|played" } else { "" }),
        )
    }

    /// `order_key`'s order without building the key: rank and suit first, the rest (formatted)
    /// only when both are equal. For hot loops.
    pub fn order_cmp(&self, other: &Card) -> std::cmp::Ordering {
        other.rank.0.cmp(&self.rank.0).then((self.suit as u8).cmp(&(other.suit as u8))).then_with(|| self.order_key().2.cmp(&other.order_key().2))
    }

    /// Whether the two cards have the same `order_key`, without building it (every field equal;
    /// `perma_bonus` as it prints: by its bits, every NaN alike).
    pub fn order_eq(&self, other: &Card) -> bool {
        let bonus = self.perma_bonus.to_bits() == other.perma_bonus.to_bits() || (self.perma_bonus.is_nan() && other.perma_bonus.is_nan());
        self.same_kind(other) && bonus && self.debuff == other.debuff && self.face_down == other.face_down && self.played_this_ante == other.played_this_ante
    }

    /// `K♠ [glass] (foil) <Red>`
    pub fn label(&self) -> String {
        // A face-down card (The House, Wheel, Mark, Fish) stays unknown: the save knows it,
        // the player doesn't, so neither does the advisor
        if self.face_down {
            return "face-down card".into();
        }
        let sym = match self.suit {
            Suit::Spades => "♠",
            Suit::Hearts => "♥",
            Suit::Clubs => "♣",
            Suit::Diamonds => "♦",
        };
        let mut s = format!("{}{}", self.rank.short(), sym);
        if let Some(e) = self.enhancement {
            s += &format!(" [{e:?}]").to_lowercase();
        }
        if let Some(e) = self.edition {
            s += &format!(" ({e:?})").to_lowercase();
        }
        if let Some(seal) = self.seal {
            s += &format!(" <{seal:?} seal>");
        }
        if self.perma_bonus != 0.0 {
            s += &format!(" +{} perma", self.perma_bonus);
        }
        if self.debuff {
            s += " DEBUFFED";
        }
        s
    }

    /// Where the game's sort puts this card, highest first (card.lua `Card:get_nominal`,
    /// cardarea.lua `CardArea:sort`: by rank, or by suit with `by_suit`). A Stone card sorts
    /// below every other in both. Left out: the tie-breaks between cards alike (the original
    /// suit's ten-thousandth, `unique_val`).
    pub fn sort_nominal(&self, by_suit: bool) -> f64 {
        let suit = match self.suit {
            Suit::Spades => 0.04,
            Suit::Hearts => 0.03,
            Suit::Clubs => 0.02,
            Suit::Diamonds => 0.01,
        };
        let face = match self.rank.0 {
            14 => 0.4,
            13 => 0.3,
            12 => 0.2,
            11 => 0.1,
            _ => 0.0,
        };
        let mult = if self.enhancement == Some(Enhancement::Stone) {
            -1000.0
        } else if by_suit {
            1000.0
        } else {
            1.0
        };
        self.rank.chips() + suit * mult + face
    }
}

/// `n` as "1st", "2nd", "3rd", "4th", … "11th", "12th", "13th", "21st"
pub(crate) fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// For each card of `hand` (as it sits, left to right), the keys (`Card::sort_nominal`) of the
/// face-up cards just below and above it in the rank sort (`[0]`) and the suit sort (`[1]`), those
/// both sides included (it may be a card alike): what the two sorts bound a face-down card by
/// (`hand_labels`, and the advice's draws of what it could be, `sim::Hidden`). In the sort the
/// hand sits in, its neighbours on screen; in the other, by key, and a face-up card with its own
/// key bounds nothing there: which of the two the game puts first is `unique_val`'s, which the
/// save doesn't give. Whether the hand sits in a sort is told by every card's real key.
pub fn sort_bounds(hand: &[Card]) -> Vec<[(f64, f64); 2]> {
    // whether the hand sits in this sort's order (highest first), as the game leaves it after a
    // sort: then where each card sits is what you see, ties between cards alike included
    let in_order = |by_suit: bool| hand.windows(2).all(|w| w[0].sort_nominal(by_suit) >= w[1].sort_nominal(by_suit));
    let sorted = [in_order(false), in_order(true)];
    let bounds = |i: usize, by_suit: bool| -> (f64, f64) {
        let key = |c: &Card| c.sort_nominal(by_suit);
        if sorted[by_suit as usize] {
            let above = hand[..i].iter().rev().find(|c| !c.face_down).map_or(f64::INFINITY, key);
            let below = hand[i + 1..].iter().find(|c| !c.face_down).map_or(f64::NEG_INFINITY, key);
            return (below, above);
        }
        let k = key(&hand[i]);
        let keys = hand.iter().filter(|c| !c.face_down).map(key);
        let below = keys.clone().filter(|&x| x < k).fold(f64::NEG_INFINITY, f64::max);
        let above = keys.filter(|&x| x > k).fold(f64::INFINITY, f64::min);
        (below, above)
    };
    (0..hand.len()).map(|i| [bounds(i, false), bounds(i, true)]).collect()
}

/// Each card of `hand` (as it sits, left to right) by what you can see of it: a face-up card by
/// its `Card::label`, a face-down one (The House, Wheel, Mark, Fish) by its place in the hand and
/// what the game's two sorts show of it ("face-down card, 1st from left: ♠ 10 to A").
///
/// The game sorts face-down cards by their real key (cardarea.lua `CardArea:sort`), so switching
/// between the rank and suit sort shows where each lands among the face-up cards (the sort only
/// reorders, so a raised card stays raised and can be followed). Those two places bound it: the
/// cards it can be are every rank and suit (or a Stone card) whose key fits between its face-up
/// neighbours in both sorts. In the sort your hand sits in, the neighbours are the ones on
/// screen; in the other they're found by key, where a face-up card alike (the same key) could
/// sit on either side (`unique_val`, not in the save), so it bounds nothing: there the name
/// can be wider than the screen shows, never narrower, and it differs with which card alike it is.
/// Whether the hand sits in a sort is told by every card's real key (a hand you rearranged by
/// hand is never bounded by an order it isn't in), the other way the hidden card reaches the
/// name: exact or wider, never wrong. When the face-up cards bound it nowhere (every card face down) or
/// bound another face-down card the same way, it's also named by its order among the face-down
/// cards in the rank sort ("the 2nd highest by the rank sort"), which the sort shows too. Never
/// the card itself: only what you can see. The boss's own rule (The Mark turns down only face
/// cards) isn't named here; the advice's draws of what the cards could be read it (`sim::Hidden`).
pub fn hand_labels(hand: &[Card]) -> Vec<String> {
    let up: Vec<&Card> = hand.iter().filter(|c| !c.face_down).collect();
    let all_bounds = sort_bounds(hand);
    let bounds = |i: usize, by_suit: bool| all_bounds[i][by_suit as usize];
    let suits = [(Suit::Spades, '♠'), (Suit::Hearts, '♥'), (Suit::Clubs, '♣'), (Suit::Diamonds, '♦')];
    // what the two sorts leave `c` to be, in words; None when they leave it anything
    let pinned = |i: usize| -> Option<String> {
        let (r, s) = (bounds(i, false), bounds(i, true));
        let fits = |x: &Card| {
            let (kr, ks) = (x.sort_nominal(false), x.sort_nominal(true));
            (r.0..=r.1).contains(&kr) && (s.0..=s.1).contains(&ks)
        };
        let mut parts = vec![];
        let mut every = true;
        for (suit, sym) in suits {
            let ranks: Vec<u8> = (2..=14).filter(|&n| fits(&Card::new(Rank(n), suit))).collect();
            every &= ranks.len() == 13;
            // one contiguous run per suit: both bounds are intervals of the same order within it
            match (ranks.first(), ranks.last()) {
                (Some(_), Some(_)) if ranks.len() == 13 => parts.push(format!("any {sym}")),
                (Some(&lo), Some(&hi)) if lo == hi => parts.push(format!("{}{sym}", Rank(lo).short())),
                (Some(&lo), Some(&hi)) => parts.push(format!("{sym} {} to {}", Rank(lo).short(), Rank(hi).short())),
                _ => {}
            }
        }
        let stone = (2..=14).any(|n| suits.iter().any(|&(suit, _)| fits(&Card { enhancement: Some(Enhancement::Stone), ..Card::new(Rank(n), suit) })));
        if every {
            // (at most a face-up Stone card below it: nothing worth a word)
            return None;
        }
        if stone {
            parts.push("a Stone card".into());
        }
        Some(parts.join(" or "))
    };
    let down: Vec<usize> = (0..hand.len()).filter(|&i| hand[i].face_down).collect();
    let pins: Vec<Option<String>> = (0..hand.len()).map(|i| if hand[i].face_down { pinned(i) } else { None }).collect();
    // highest first by the rank sort; cards alike keep their places' order
    let mut by_rank = down.clone();
    by_rank.sort_by(|&a, &b| hand[b].sort_nominal(false).total_cmp(&hand[a].sort_nominal(false)).then(a.cmp(&b)));
    (0..hand.len())
        .map(|i| {
            if !hand[i].face_down {
                return hand[i].label();
            }
            let mut what = vec![];
            if let Some(p) = &pins[i] {
                what.push(p.clone());
            }
            let shared = down.iter().any(|&j| j != i && pins[j] == pins[i]);
            if shared || pins[i].is_none() {
                let k = by_rank.iter().position(|&j| j == i).unwrap_or(0);
                let nth = if k == 0 { "the highest".into() } else { format!("the {} highest", ordinal(k + 1)) };
                // among the face-down cards (with every card face down, of them all)
                let of = if up.is_empty() { "" } else { " face-down card" };
                what.push(format!("{nth}{of} by the rank sort"));
            }
            format!("face-down card, {} from left: {}", ordinal(i + 1), what.join(", "))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stake {
    White = 1,
    Red,
    Green,
    Black,
    Blue,
    Purple,
    Orange,
    Gold,
}

impl Stake {
    pub fn from_number(n: i64) -> Option<Stake> {
        use Stake::*;
        Some([White, Red, Green, Black, Blue, Purple, Orange, Gold][usize::try_from(n - 1).ok().filter(|&i| i < 8)?])
    }

    pub fn number(self) -> u8 {
        self as u8
    }
}

/// Poker hand names exactly as the save uses them (`GAME.hands` keys).
pub const POKER_HANDS: [&str; 12] = [
    "Flush Five",
    "Flush House",
    "Five of a Kind",
    "Straight Flush",
    "Four of a Kind",
    "Full House",
    "Flush",
    "Straight",
    "Three of a Kind",
    "Two Pair",
    "Pair",
    "High Card",
];

impl Card {
    /// Parses `KH`, `10S:glass:foil:red`, `AD:stone`, `5C:+30`.
    /// Modifiers: bonus mult wild glass steel stone gold lucky · foil holo poly ·
    /// red blue goldseal purple · debuff · +N (perma chips).
    pub fn parse(token: &str) -> Result<Card, String> {
        let mut parts = token.split(':');
        let body = parts.next().unwrap_or_default().to_ascii_uppercase();
        if body.len() < 2 {
            return Err(format!("bad card '{token}'"));
        }
        let (r, s) = body.split_at(body.len() - 1);
        let rank = match r {
            "A" => 14,
            "K" => 13,
            "Q" => 12,
            "J" => 11,
            "T" => 10,
            n => n.parse().ok().filter(|n| (2..=10).contains(n)).ok_or_else(|| format!("bad rank in '{token}'"))?,
        };
        let suit = match s {
            "S" => Suit::Spades,
            "H" => Suit::Hearts,
            "C" => Suit::Clubs,
            "D" => Suit::Diamonds,
            _ => return Err(format!("bad suit in '{token}'")),
        };
        let mut c = Card::new(Rank(rank), suit);
        for m in parts {
            match m.to_ascii_lowercase().as_str() {
                "bonus" => c.enhancement = Some(Enhancement::Bonus),
                "mult" => c.enhancement = Some(Enhancement::Mult),
                "wild" => c.enhancement = Some(Enhancement::Wild),
                "glass" => c.enhancement = Some(Enhancement::Glass),
                "steel" => c.enhancement = Some(Enhancement::Steel),
                "stone" => c.enhancement = Some(Enhancement::Stone),
                "gold" => c.enhancement = Some(Enhancement::Gold),
                "lucky" => c.enhancement = Some(Enhancement::Lucky),
                "foil" => c.edition = Some(Edition::Foil),
                "holo" => c.edition = Some(Edition::Holo),
                "poly" | "polychrome" => c.edition = Some(Edition::Polychrome),
                "red" => c.seal = Some(Seal::Red),
                "blue" => c.seal = Some(Seal::Blue),
                "goldseal" => c.seal = Some(Seal::Gold),
                "purple" => c.seal = Some(Seal::Purple),
                "debuff" => c.debuff = true,
                p if p.starts_with('+') => {
                    c.perma_bonus = p[1..].parse().map_err(|_| format!("bad perma bonus in '{token}'"))?
                }
                other => return Err(format!("unknown modifier '{other}' in '{token}'")),
            }
        }
        Ok(c)
    }

    /// Space-separated list of `Card::parse` tokens.
    pub fn parse_list(spec: &str) -> Result<Vec<Card>, String> {
        spec.split_whitespace().map(Card::parse).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Card::parse_list` tokens, a leading `?` turning that card face down
    fn hand(spec: &str) -> Vec<Card> {
        spec.split_whitespace()
            .map(|t| {
                let mut c = Card::parse(t.trim_start_matches('?')).unwrap();
                c.face_down = t.starts_with('?');
                c
            })
            .collect()
    }

    #[test]
    fn a_face_down_card_is_named_by_its_place_and_what_the_sorts_show() {
        // The Mark at Ante 4 (run Q84UC3NR, docs/design.md gap 25), sorted by suit: ? 9♠ 8♠ 4♠
        // ? 9♥ 2♥ ?. The leftmost lands above 9♠ in both sorts, so it's a spade from 9 (a
        // second 9♠) up; the next sits between 4♠ and 9♥ by suit and above every face-up card
        // by rank, so a heart from 10; the last is below 2♥ by suit and above 9♠ by rank: a
        // Club or Diamond from 10. Named only by that: never the card (K♠, J♥, Q♣).
        let h = hand("?KS 9S 8S 4S ?JH 9H 2H ?QC");
        let names = hand_labels(&h);
        assert_eq!(names[0], "face-down card, 1st from left: ♠ 9 to A");
        assert_eq!(names[4], "face-down card, 5th from left: ♥ 10 to A");
        assert_eq!(names[7], "face-down card, 8th from left: ♣ 10 to A or ♦ 10 to A");
        assert_eq!(names[1], "9♠");
        // the same cards in the rank sort (K♠ Q♣ J♥ by their keys): each keeps what it is, at
        // its new place
        let h2 = hand("?KS ?QC ?JH 9S 9H 8S 4S 2H");
        let names2 = hand_labels(&h2);
        assert_eq!(names2[0], "face-down card, 1st from left: ♠ 9 to A");
        assert_eq!(names2[1], "face-down card, 2nd from left: ♣ 10 to A or ♦ 10 to A");
        assert_eq!(names2[2], "face-down card, 3rd from left: ♥ 10 to A");
    }

    #[test]
    fn a_face_down_card_next_to_one_alike_is_named_by_where_it_sits() {
        // 10♠ ? 9♠ 8♠ in both sorts: whether it's the second 9♠ or the second 10♠, it sits
        // between them on screen, and that's all its name says (by key alone each would be
        // bounded differently: a name that told them apart would read the hidden card)
        for real in ["?9S", "?TS"] {
            let h = hand(&format!("TS {real} 9S 8S"));
            assert_eq!(hand_labels(&h)[1], "face-down card, 2nd from left: ♠ 9 to 10", "{real}");
        }
    }

    #[test]
    fn face_down_cards_the_sorts_cant_tell_apart_are_named_by_their_order() {
        // every card face down (The House's first hand): nothing bounds them but their order
        // among themselves, which the rank sort shows
        let h = hand("?7C ?AS ?2S ?KH ?7D");
        let names = hand_labels(&h);
        assert_eq!(
            names,
            [
                "face-down card, 1st from left: the 3rd highest by the rank sort",
                "face-down card, 2nd from left: the highest by the rank sort",
                "face-down card, 3rd from left: the 5th highest by the rank sort",
                "face-down card, 4th from left: the 2nd highest by the rank sort",
                "face-down card, 5th from left: the 4th highest by the rank sort",
            ]
        );
        // two in the same gap between face-up cards (above 9♥, below A♠ in both sorts): the
        // gap, and their order among the face-down cards
        let h = hand("?QS AS ?JS 9H");
        let names = hand_labels(&h);
        assert_eq!(names[0], "face-down card, 1st from left: ♠ 9 to A or ♥ 9 to A, the highest face-down card by the rank sort");
        assert_eq!(names[2], "face-down card, 3rd from left: ♠ 9 to A or ♥ 9 to A, the 2nd highest face-down card by the rank sort");
    }

    #[test]
    fn a_stone_card_sorts_last_in_both_orders() {
        // card.lua `Card:get_nominal`: a Stone card's key is below every other card's in both
        // sorts, so one face down at the bottom of both might be one
        let h = hand("?3H:stone 2D 5C");
        assert_eq!(hand_labels(&h)[0], "face-down card, 1st from left: 2♦ or a Stone card");
        // next to a face-up card alike, it may sort on either side of it (`unique_val`), so that
        // card bounds nothing
        let h = hand("?2D 2D 5C");
        assert_eq!(hand_labels(&h)[0], "face-down card, 1st from left: ♣ 2 to 5 or ♦ 2 to 5 or a Stone card");
        // above every card but a face-up Stone card: the sorts say nothing, so its order does
        let h = hand("?8H 3H:stone");
        assert_eq!(hand_labels(&h)[0], "face-down card, 1st from left: the highest face-down card by the rank sort");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(22), "22nd");
    }
}
