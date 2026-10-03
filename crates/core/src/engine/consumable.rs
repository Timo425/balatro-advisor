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

impl CardEffect {
    pub fn label(self) -> String {
        match self {
            CardEffect::Enhance(e) => format!("{e:?}"),
            CardEffect::Seal(x) => format!("{x:?} Seal"),
            CardEffect::Suit(x) => format!("→ {}", x.name()),
            CardEffect::UpRank => "+1 rank".into(),
            CardEffect::CopyLeftToRight => "the first becomes a copy of the second".into(),
            CardEffect::Destroy => "destroys".into(),
            CardEffect::Copies(k) => format!("{k} copies"),
        }
    }
}

/// Whether the simulation plays out what the effect changes. A Purple Seal's tarot when
/// discarded isn't simulated, so it can't be valued.
pub fn modelled(effect: CardEffect) -> bool {
    effect != CardEffect::Seal(Seal::Purple)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameData;

    fn effect(key: &str) -> (CardEffect, usize, usize) {
        card_effect(key, &GameData::bundled().center(key).unwrap().config).unwrap()
    }

    #[test]
    fn effects_follow_the_game() {
        let deck = Card::parse_list("AS 5H KD 2C").unwrap();
        // Death (2 cards exactly): the first becomes a copy of the second
        let (death, min, max) = effect("c_death");
        assert_eq!((death, min, max), (CardEffect::CopyLeftToRight, 2, 2));
        assert_eq!(apply(death, &deck, &[1, 2])[1].label(), "K♦");
        // Strength: +1 rank, an Ace wraps to 2
        let (strength, _, max) = effect("c_strength");
        assert_eq!(max, 2);
        let up = apply(strength, &deck, &[0, 1]);
        assert_eq!((up[0].rank.0, up[1].rank.0), (2, 6));
        // Cryptid: two copies added
        let (cryptid, _, _) = effect("c_cryptid");
        let d = apply(cryptid, &deck, &[2]);
        assert_eq!(d.len(), 6);
        assert_eq!((d[4].label(), d[5].label()), ("K♦".to_string(), "K♦".to_string()));
        // The Hanged Man: destroys
        let (hanged, _, _) = effect("c_hanged_man");
        assert_eq!(apply(hanged, &deck, &[0, 3]).iter().map(Card::label).collect::<Vec<_>>(), vec!["5♥", "K♦"]);
        // seals, suits, enhancements
        assert_eq!(effect("c_trance").0, CardEffect::Seal(Seal::Blue));
        assert_eq!(effect("c_world").0, CardEffect::Suit(Suit::Spades));
        assert_eq!(effect("c_chariot").0, CardEffect::Enhance(Enhancement::Steel));
        // random effects aren't here
        assert!(data_effect("c_aura").is_none() && data_effect("c_familiar").is_none());
    }

    fn data_effect(key: &str) -> Option<(CardEffect, usize, usize)> {
        card_effect(key, &GameData::bundled().center(key).unwrap().config)
    }
}
