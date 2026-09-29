#![recursion_limit = "256"]
//! balatro-advisor: read a vanilla Balatro save, score hands, value jokers.

pub mod advise;
pub mod bench;
pub mod calibration;
pub mod data;
pub mod describe;
pub mod engine;
pub mod gold;
pub mod golden;
pub mod jkr;
pub mod lua;
pub mod model;
pub mod paths;
pub mod save;
pub mod sim;

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
