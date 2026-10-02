//! What a consumable does to the cards you pick (card.lua `Card:use_consumeable`), read from its
//! game data (`config`): the effects that are certain once you've chosen the cards. Random ones
//! (Aura's edition, Familiar's new cards, Sigil's suit) aren't here.

use crate::model::{Card, Enhancement, Rank, Seal, Suit};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CardEffect {
    /// `mod_conv` = an enhancement key (The Magician, The Chariot, …)
    Enhance(Enhancement),
    /// `extra` = a seal name (Talisman, Deja Vu, Trance, Medium)
    Seal(Seal),
    /// `suit_conv` (The Star, The Moon, The Sun, The World)
    Suit(Suit),
    /// `mod_conv = "up_rank"` (Strength): +1 rank, Ace wraps to 2
    UpRank,
    /// `mod_conv = "card"` (Death): the left card becomes a copy of the right one
    CopyLeftToRight,
    /// `remove_card` with chosen cards (The Hanged Man)
    Destroy,
    /// Cryptid: `extra` copies of the card
    Copies(usize),
}

/// A consumable's effect on chosen cards, and how many cards it takes (at least, at most).
pub fn card_effect(key: &str, config: &serde_json::Value) -> Option<(CardEffect, usize, usize)> {
    let max = config.get("max_highlighted").and_then(|v| v.as_u64())? as usize;
    let min = config.get("min_highlighted").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let effect = if let Some(m) = config.get("mod_conv").and_then(|v| v.as_str()) {
        match m {
            "up_rank" => CardEffect::UpRank,
            "card" => CardEffect::CopyLeftToRight,
            _ => CardEffect::Enhance(Enhancement::from_key(m)?),
        }
    } else if let Some(s) = config.get("suit_conv").and_then(|v| v.as_str()) {
        CardEffect::Suit(Suit::from_name(s)?)
    } else if config.get("remove_card").and_then(|v| v.as_bool()).unwrap_or(false) {
        CardEffect::Destroy
    } else if let Some(s) = config.get("extra").and_then(|v| v.as_str()) {
        CardEffect::Seal(Seal::from_name(s)?)
    } else if key == "c_cryptid" {
        CardEffect::Copies(config.get("extra").and_then(|v| v.as_u64()).unwrap_or(2) as usize)
    } else {
        return None;
    };
    Some((effect, min.max(1), max.max(1)))
}

/// `deck` with the effect used on the cards at `targets` (in the order you'd pick them; for
/// Death the left one first). Copies are added at the end; destroyed cards are removed.
pub fn apply(effect: CardEffect, deck: &[Card], targets: &[usize]) -> Vec<Card> {
    let mut d = deck.to_vec();
    match effect {
        CardEffect::Enhance(e) => targets.iter().for_each(|&i| d[i].enhancement = Some(e)),
        CardEffect::Seal(s) => targets.iter().for_each(|&i| d[i].seal = Some(s)),
        CardEffect::Suit(s) => targets.iter().for_each(|&i| d[i].suit = s),
        CardEffect::UpRank => targets.iter().for_each(|&i| d[i].rank = Rank(if d[i].rank.0 >= 14 { 2 } else { d[i].rank.0 + 1 })),
        CardEffect::CopyLeftToRight => {
            if let [l, r] = targets {
                d[*l] = d[*r];
            }
        }
        CardEffect::Destroy => {
            let mut gone = targets.to_vec();
            gone.sort_unstable_by(|a, b| b.cmp(a));
            for i in gone {
                d.remove(i);
            }
        }
        CardEffect::Copies(n) => {
            if let Some(&i) = targets.first() {
                let c = d[i];
                d.extend(std::iter::repeat_n(c, n));
            }
        }
    }
    d
}
