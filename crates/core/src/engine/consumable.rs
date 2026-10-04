//! What a consumable does to the cards you pick (card.lua `Card:use_consumeable`), read from its
//! game data (`config`), and the jokers that do the same to a card each round (DNA). An effect
//! with a random result on the chosen cards (Aura's edition) is listed as its outcomes, each
//! with its chance (`card_outcomes`). Random effects on cards nobody picks (Familiar's new
//! cards, Sigil's suit, Immolate's destroyed cards) aren't here.

use crate::model::{Card, Edition, Enhancement, Rank, Seal, Suit};

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
    /// An edition on the card (one of Aura's outcomes)
    Edition(Edition),
}

/// The edition `poll_edition(key, nil, no_negative, guaranteed)` gives (common_events.lua: the
/// poll's thresholds 1 − 0.006·25 for Polychrome and 1 − 0.02·25 for Holo, Foil below): Aura's
/// edition, and The Wheel of Fortune's on a joker. Chances sum to 1.
pub const GUARANTEED_EDITION: [(f64, Edition); 3] = [(0.15, Edition::Polychrome), (0.35, Edition::Holo), (0.5, Edition::Foil)];

/// An effect's outcomes, each with its chance
pub type Outcomes = Vec<(f64, CardEffect)>;

/// A consumable's effect on chosen cards as its outcomes, each with its chance (one, certain,
/// for every effect `card_effect` reads; Aura's edition: `GUARANTEED_EDITION` on one card, a
/// game fact: its `config` is empty, card.lua `Card:use_consumeable`), and how many cards it
/// takes (at least, at most).
pub fn card_outcomes(key: &str, config: &serde_json::Value) -> Option<(Outcomes, usize, usize)> {
    if key == "c_aura" {
        return Some((GUARANTEED_EDITION.iter().map(|&(w, e)| (w, CardEffect::Edition(e))).collect(), 1, 1));
    }
    let (e, min, max) = card_effect(key, config)?;
    Some((vec![(1.0, e)], min, max))
}

/// A joker's effect on a card you pick each round, as a consumable's (`card_effect`), and how
/// many cards: DNA (card.lua `Card:calculate_joker`, `context.before`): when the first hand of
/// the round is a single card, a copy of it is added to your deck and hand (`copy_card`, so its
/// enhancement, seal and edition too).
pub fn round_card_effect(key: &str) -> Option<(CardEffect, usize, usize)> {
    (key == "j_dna").then_some((CardEffect::Copies(1), 1, 1))
}

/// Whether the effect can go on `card` (card.lua `Card:can_use_consumeable`: Aura only on a
/// card without an edition)
pub fn can_target(effect: CardEffect, card: &Card) -> bool {
    !matches!(effect, CardEffect::Edition(_)) || card.edition.is_none()
}

/// What a set of outcomes does, for a note ("Steel", "Polychrome 15% / Holo 35% / Foil 50%")
pub fn outcomes_label(outcomes: &[(f64, CardEffect)]) -> String {
    match outcomes {
        [(_, e)] => e.label(),
        _ => outcomes.iter().map(|(w, e)| format!("{} {:.0}%", e.label(), w * 100.0)).collect::<Vec<_>>().join(" / "),
    }
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
            CardEffect::Edition(e) => format!("{e:?}"),
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
        CardEffect::Edition(e) => targets.iter().for_each(|&i| d[i].edition = Some(e)),
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
        // random effects on cards nobody picks aren't here
        assert!(data_effect("c_familiar").is_none() && data_effect("c_sigil").is_none());
    }

    #[test]
    fn aura_is_a_random_edition_on_one_card_without_one() {
        // fails if Aura isn't searched (no outcomes), or its chances aren't the game's
        let (outs, min, max) = card_outcomes("c_aura", &GameData::bundled().center("c_aura").unwrap().config).unwrap();
        assert_eq!((min, max), (1, 1));
        assert_eq!(outs, vec![(0.15, CardEffect::Edition(Edition::Polychrome)), (0.35, CardEffect::Edition(Edition::Holo)), (0.5, CardEffect::Edition(Edition::Foil))]);
        let deck = Card::parse_list("AS 5H").unwrap();
        assert_eq!(apply(outs[0].1, &deck, &[1])[1].edition, Some(Edition::Polychrome));
        // only a card without an edition (card.lua can_use_consumeable)
        let mut foil = deck[0];
        foil.edition = Some(Edition::Foil);
        assert!(!can_target(outs[2].1, &foil) && can_target(outs[2].1, &deck[0]));
        // a certain effect is one outcome
        assert_eq!(card_outcomes("c_chariot", &GameData::bundled().center("c_chariot").unwrap().config).unwrap().0, vec![(1.0, CardEffect::Enhance(Enhancement::Steel))]);
        // DNA copies one card a round, as Cryptid does with one copy
        assert_eq!(round_card_effect("j_dna"), Some((CardEffect::Copies(1), 1, 1)));
    }

    fn data_effect(key: &str) -> Option<(CardEffect, usize, usize)> {
        card_effect(key, &GameData::bundled().center(key).unwrap().config)
    }
}
