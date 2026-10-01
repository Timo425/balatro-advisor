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
