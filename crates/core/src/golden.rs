//! Golden cases: a real hand with the score the game showed.
//!
//! Stored as JSON in `tests/golden/`. The board is captured from the save before the hand
//! is played (`score --hand … --golden NAME`); the real score is added after
//! (`golden set NAME SCORE`). `cargo test` checks every case that has one.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::{self, Board, Unlucky};
use crate::model::Card;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub board: Board,
    pub played: Vec<Card>,
    pub held: Vec<Card>,
    /// What the engine said when the case was captured.
    pub predicted: f64,
    /// What the game showed. `None` until the owner fills it in.
    pub expected: Option<f64>,
    #[serde(default)]
    pub note: String,
}

impl Case {
    pub fn path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.json"))
    }

    pub fn load(path: &Path) -> Result<Case, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, dir: &Path) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let p = Case::path(dir, &self.name);
        std::fs::write(&p, serde_json::to_string_pretty(self).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
        Ok(p)
    }

    /// Engine score with every random roll failing (golden hands should avoid luck), with the
    /// cards in the order they were played and held.
    pub fn rescore(&self) -> f64 {
        engine::score_as_played(&self.board, &self.played, &self.held, &mut Unlucky, false).score
    }
}

/// All cases in `dir`, sorted by file name.
pub fn load_all(dir: &Path) -> Vec<(PathBuf, Result<Case, String>)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    paths.sort();
    paths.into_iter().map(|p| { let c = Case::load(&p); (p, c) }).collect()
}
