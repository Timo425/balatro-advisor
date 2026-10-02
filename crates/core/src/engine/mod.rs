//! Scoring engine: hand detection and the scoring pass, ported from the game source.

pub mod board;
pub mod consumable;
pub mod hand;
pub mod joker;
pub mod rng;
pub mod score;

pub use hand::{HandInfo, HandType, RuleFlags};
pub use joker::{Joker, Kind};
pub use rng::{Lucky, Rng, Rolls, Unlucky};
pub use score::{discard_money, BlindRules, Board, Level, Outcome, Step, score};
#[cfg(test)]
mod tests;
