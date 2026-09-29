//! Gold Stake sticker progress from `profile.jkr`.
//!
//! Game rule (misc_functions.lua): `set_joker_win` adds 1 to
//! `joker_usage[key].wins[stake]` for every joker in the slots on a win, and
//! `get_joker_win_sticker` shows the sticker for the highest stake key present.
//! So Gold ⇔ `8 ∈ wins`. Jokers never used have no `joker_usage` entry at all.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::data::{GameData, RARITY_NAMES};
use crate::lua::{Key, Value};
use crate::Error;

#[derive(Debug, Clone, Serialize)]
pub struct JokerProgress {
    pub key: String,
    pub name: String,
    pub rarity: String,
    /// Highest stake won with (0 = none), i.e. the sticker shown in the collection.
    pub best_stake: u8,
    pub times_used: i64,
    pub wins: BTreeMap<u8, i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoldReport {
    pub profile: u8,
    pub have_gold: usize,
    pub total: usize,
    /// Every joker without a Gold sticker, collection order.
    pub missing: Vec<JokerProgress>,
    /// Our sum of best stakes vs the game's `progress.joker_stickers.tally`.
    /// A mismatch means the format changed and the report can't be trusted.
    pub tally_computed: i64,
    pub tally_in_file: Option<i64>,
}

impl GoldReport {
    pub fn tally_ok(&self) -> bool {
        self.tally_in_file.is_none_or(|t| t == self.tally_computed)
    }

    pub fn is_missing(&self, key: &str) -> bool {
        self.missing.iter().any(|j| j.key == key)
    }
}

pub fn load(save_dir: &Path, profile: u8, data: &GameData) -> Result<GoldReport, Error> {
    let path = save_dir.join(profile.to_string()).join("profile.jkr");
    let v = crate::jkr::read(&path)?;
    Ok(report(&v, profile, data))
}

pub fn report(profile_v: &Value, profile: u8, data: &GameData) -> GoldReport {
    let usage = profile_v.get("joker_usage");
    let mut missing = Vec::new();
    let mut have_gold = 0;
    let mut tally = 0i64;
    let mut total = 0;
    for j in data.jokers() {
        total += 1;
        let u = usage.get(&j.key);
        let wins: BTreeMap<u8, i64> = u
            .get("wins")
            .table()
            .map(|t| {
                t.entries
                    .iter()
                    .filter_map(|(k, v)| match k {
                        Key::Int(s) => Some((*s as u8, v.int().unwrap_or(0))),
                        Key::Str(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let best = wins.keys().copied().max().unwrap_or(0);
        tally += i64::from(best);
        if best >= 8 {
            have_gold += 1;
        } else {
            missing.push(JokerProgress {
                key: j.key.clone(),
                name: j.name.clone(),
                rarity: RARITY_NAMES[j.rarity.unwrap_or(0).min(4) as usize].to_string(),
                best_stake: best,
                times_used: u.get("count").int().unwrap_or(0),
                wins,
            });
        }
    }
    GoldReport {
        profile,
        have_gold,
        total,
        missing,
        tally_computed: tally,
        tally_in_file: profile_v.at("progress.joker_stickers.tally").int(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_means_stake_8_in_wins() {
        let v = crate::lua::parse(
            r#"return {["joker_usage"]={
                ["j_joker"]={["count"]=5,["wins"]={[8]=1,[2]=3,},},
                ["j_jolly"]={["count"]=2,["wins"]={[7]=1,},},
              },["progress"]={["joker_stickers"]={["tally"]=15,["of"]=1200,},},}"#,
        )
        .unwrap();
        let r = report(&v, 1, GameData::bundled());
        assert_eq!(r.total, 150);
        assert_eq!(r.have_gold, 1);
        assert!(!r.is_missing("j_joker"));
        let jolly = r.missing.iter().find(|j| j.key == "j_jolly").unwrap();
        assert_eq!(jolly.best_stake, 7);
        // Never-used jokers are missing too
        assert!(r.is_missing("j_baron"));
        assert_eq!(r.tally_computed, 15);
        assert!(r.tally_ok());
    }
}
