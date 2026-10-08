//! Scoring engine: hand detection and the scoring pass, ported from the game source.

pub mod board;
pub mod consumable;
pub mod hand;
pub mod joker;
pub mod rng;
pub mod score;

pub use hand::{HandInfo, HandType, RuleFlags};
pub use joker::{Joker, Kind, RunEvent};
pub use rng::{Lucky, Rng, Rolls, Unlucky};
pub use score::{discard_money, won_round_money, BlindRules, Board, Level, Outcome, Step, score, score_as_played};
#[cfg(test)]
mod tests;
