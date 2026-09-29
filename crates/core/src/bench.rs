//! Fixed synthetic boards for timing (`balatro-advisor bench`).

use std::time::Instant;

use crate::data::GameData;
use crate::engine::{Board, Joker};
use crate::model::{Card, Rank, Suit};
use crate::sim::{self, RoundStart};

pub fn standard_deck() -> Vec<Card> {
    Suit::ALL.iter().flat_map(|&s| (2..=14).map(move |r| Card::new(Rank(r), s))).collect()
}

pub fn sample_board(keys: &[&str]) -> Board {
    let mut b = Board::empty();
    b.jokers = keys.iter().filter_map(|k| Joker::from_key(k, GameData::bundled())).collect();
    b
}

#[derive(Debug, serde::Serialize)]
pub struct Timing {
    pub name: String,
    pub ms: f64,
    pub per_item_us: f64,
}

pub fn run() -> Vec<Timing> {
    let deck = standard_deck();
    let b = sample_board(&["j_joker", "j_jolly", "j_blueprint", "j_cavendish", "j_baron"]);
    let mut out = Vec::new();
    let mut time = |name: &str, n: usize, f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        out.push(Timing { name: name.into(), ms, per_item_us: ms * 1000.0 / n as f64 });
    };
    let hand: Vec<Card> = deck[..8].to_vec();
    time("best_play x1000 (8-card hand, 5 jokers)", 1000, &mut || {
        for _ in 0..1000 {
            std::hint::black_box(sim::best_play(&b, &hand));
        }
    });
    time("typical_hands 1000 samples", 1000, &mut || {
        std::hint::black_box(sim::typical_hands(&b, &deck, 8, 1000, 1));
    });
    let start = RoundStart { hand: vec![], deck: deck.clone(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 3000.0 };
    time("round_odds 200 sims", 200, &mut || {
        std::hint::black_box(sim::round_odds(&b, &start, 200, 1));
    });
    out
}
