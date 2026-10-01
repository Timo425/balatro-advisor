//! Building a scoring `Board` from a parsed save.

use super::hand::HandType;
use super::joker::Joker;
use super::score::{BlindRules, Board, Level};
use crate::data::GameData;
use crate::model::{Card, Enhancement, Rank, Suit};
use crate::save::RunState;

impl Board {
    pub fn from_run(s: &RunState, data: &GameData) -> Board {
        let mut levels = HandType::ALL.map(Level::base);
        for (name, h) in &s.hand_levels {
            if let Some(t) = HandType::from_name(name) {
                levels[t as usize] = Level {
                    level: h.level,
                    chips: h.chips,
                    mult: h.mult,
                    s_chips: h.s_chips,
                    s_mult: h.s_mult,
                    l_chips: h.l_chips,
                    l_mult: h.l_mult,
                    played: h.played,
                    played_this_round: h.played_this_round,
                    visible: h.visible,
                };
            }
        }
        let full = s.full_deck();
        let tally = |f: &dyn Fn(&Card) -> bool| full.iter().filter(|c| f(c)).count() as i64;
        let blind = s.current_blind.as_ref().map_or_else(BlindRules::default, |b| BlindRules {
            key: b.key.clone(),
            disabled: b.disabled,
            eye_seen: b.hands_seen.iter().filter_map(|h| HandType::from_name(h)).fold(0, |m, h| m | h.bit()),
            mouth_only: b.only_hand.as_deref().and_then(HandType::from_name),
        });
        Board {
            jokers: s.jokers.iter().map(|j| Joker::from_save(j, data)).collect(),
            joker_slots: s.joker_slots,
            levels,
            planets_held: s
                .consumables
                .iter()
                .filter(|c| c.set == "Planet")
                .filter_map(|c| {
                    let hand = data.center(&c.key)?.config.get("hand_type")?.as_str()?.to_string();
                    HandType::from_name(&hand)
                })
                .collect(),
            observatory: s.vouchers.iter().any(|v| v == "v_observatory"),
            hands_left: s.hands_left,
            discards_left: s.discards_left,
            dollars: s.dollars,
            skips: s.skips,
            hands_played: s.hands_played,
            tarots_used: s.tarots_used,
            starting_deck_size: s.starting_deck_size,
            playing_cards: full.len() as i64,
            deck_remaining: s.draw_pile.len() as i64,
            steel_tally: tally(&|c| c.enhancement == Some(Enhancement::Steel)),
            stone_tally: tally(&|c| c.enhancement == Some(Enhancement::Stone)),
            driver_tally: tally(&|c| c.enhancement.is_some()),
            probability: s.probability_normal,
            idol: s.round_targets.idol.map(|(r, suit): (Rank, Suit)| (r.0, suit)),
            ancient_suit: s.round_targets.ancient_suit,
            most_played: HandType::from_name(&s.most_played_hand),
            blind,
            plasma: s.deck == "Plasma Deck",
            mail_rank: s.round_targets.mail_rank.map(|r| r.0),
            seal_seen_value: 0.0,
            planet_slots: (s.consumable_slots - s.consumables.len() as i64).max(0),
        }
    }
}

impl RunState {
    /// Every playing card the run owns (draw pile, hand, discard).
    pub fn full_deck(&self) -> Vec<Card> {
        self.draw_pile.iter().chain(&self.hand).chain(&self.discard_pile).copied().collect()
    }
}
