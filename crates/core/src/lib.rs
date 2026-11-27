#![recursion_limit = "256"]
//! balatro-advisor: read a vanilla Balatro save, score hands, value jokers.

pub mod advise;
pub mod bench;
pub mod calibration;
pub mod choices;
pub mod data;
pub mod describe;
pub mod engine;
pub mod gold;
pub mod golden;
pub mod handlog;
pub mod jkr;
pub mod lua;
pub mod model;
pub mod overrides;
pub mod paths;
pub mod save;
pub mod sim;
pub mod plan;
pub mod progress;
pub mod whatif;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("could not inflate .jkr: {0}")]
    Decode(String),
    #[error(transparent)]
    Lua(#[from] lua::ParseError),
    #[error("unexpected save format: {0}")]
    Format(String),
    #[error("no run in progress ({0} does not exist)")]
    NoRun(PathBuf),
    #[error("Balatro save directory not found; set --save-dir or BALATRO_DIR. Tried: {0:?}")]
    SaveDirNotFound(Vec<PathBuf>),
}

#[cfg(test)]
mod tests {
    /// Step 4's test run is `cargo test`, not `cargo test --release` (docs/decisions.md D13): only
    /// the test profile panics on an integer overflow, where release wraps it silently
    #[test]
    fn tests_run_with_overflow_checks() {
        let max = std::hint::black_box(u8::MAX);
        let wrapped = std::panic::catch_unwind(|| max + 1).is_ok();
        assert!(!wrapped, "integer overflow wrapped: run `cargo test`, not `cargo test --release` (D13); `--release` is for the slower checks given with `--test <name> -- --ignored`");
    }
}
