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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "is_zero")]
    pub perma_bonus: f64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub debuff: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub face_down: bool,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl Card {
    pub fn new(rank: Rank, suit: Suit) -> Card {
        Card { rank, suit, enhancement: None, edition: None, seal: None, perma_bonus: 0.0, debuff: false, face_down: false }
    }

    /// `K♠ [glass] (foil) <Red>`
    pub fn label(&self) -> String {
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
