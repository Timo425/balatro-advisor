//! Scoring engine: hand detection and the scoring pass, ported from the game source.

pub mod board;
pub mod consumable;
pub mod hand;
pub mod joker;
pub mod rng;
pub mod score;

pub use hand::{HandInfo, HandType, RuleFlags};
pub use joker::{grows_in_rounds, Joker, Kind, RunEvent, CHANGE_IN_ROUND, GROWS_BY_TAKING_CARDS};
pub use rng::{Given, Lucky, Pick, Rng, Rolls, Unlucky};
pub use score::{discard_money, won_round_money, BlindRules, Board, Level, Outcome, Step, hook_pick_matters, hook_sets, hook_takes, score, score_as_played, score_pressed};
#[cfg(test)]
mod tests;
