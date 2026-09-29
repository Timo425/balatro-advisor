//! Game data (jokers, blinds, other centers).
//!
//! `data/game.json` is generated from the owner's local game install with
//! `balatro-advisor extract-data` (see README) and committed: it holds values only
//! (names, rarities, costs, config numbers), never game code.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::lua;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Center {
    pub key: String,
    pub name: String,
    pub set: String,
    #[serde(default)]
    pub order: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rarity: Option<u8>,
    #[serde(default)]
    pub cost: i64,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blueprint_compat: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eternal_compat: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perishable_compat: Option<bool>,
    /// Only offered when the deck has a card with this enhancement (Steel Joker, Lucky Cat, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhancement_gate: Option<String>,
    /// Only offered once this pool flag is set (Cavendish after Gros Michel dies).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yes_pool_flag: Option<String>,
    /// Not offered once this pool flag is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_pool_flag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blind {
    pub key: String,
    pub name: String,
    pub mult: f64,
    pub dollars: i64,
    #[serde(default)]
    pub boss: serde_json::Value,
    #[serde(default)]
    pub debuff: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ante: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameData {
    pub game_version: String,
    pub centers: Vec<Center>,
    pub blinds: Vec<Blind>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(skip)]
    index: HashMap<String, usize>,
}

pub const RARITY_NAMES: [&str; 5] = ["?", "Common", "Uncommon", "Rare", "Legendary"];

impl GameData {
    pub fn from_json(s: &str) -> serde_json::Result<GameData> {
        let mut d: GameData = serde_json::from_str(s)?;
        d.reindex();
        Ok(d)
    }

    fn reindex(&mut self) {
        self.index = self.centers.iter().enumerate().map(|(i, c)| (c.key.clone(), i)).collect();
    }

    /// The copy bundled into the binary at build time.
    pub fn bundled() -> &'static GameData {
        static DATA: OnceLock<GameData> = OnceLock::new();
        DATA.get_or_init(|| {
            GameData::from_json(include_str!("../../../data/game.json")).expect("bundled data/game.json is valid")
        })
    }

    pub fn center(&self, key: &str) -> Option<&Center> {
        self.index.get(key).map(|&i| &self.centers[i])
    }

    pub fn tag(&self, key: &str) -> Option<&Tag> {
        self.tags.iter().find(|t| t.key == key)
    }

    pub fn blind(&self, key: &str) -> Option<&Blind> {
        self.blinds.iter().find(|b| b.key == key)
    }

    /// All jokers in collection order.
    pub fn jokers(&self) -> impl Iterator<Item = &Center> {
        self.centers.iter().filter(|c| c.set == "Joker")
    }

    /// Display name for any key, falling back to the key itself.
    pub fn name<'a>(&'a self, key: &'a str) -> &'a str {
        self.center(key).map_or(key, |c| c.name.as_str())
    }
}

/// Builds `GameData` from the game's `game.lua`. Every `P_CENTERS`/`P_BLINDS`
/// entry sits on one line (`key = {...},`), which is what this relies on.
pub fn extract_from_game_lua(src: &str, game_version: &str) -> Result<GameData, String> {
    let mut centers = Vec::new();
    let mut blinds = Vec::new();
    let mut tags = Vec::new();
    let mut section = "";
    for (lineno, line) in src.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("self.P_CENTERS = {") {
            section = "centers";
            continue;
        }
        if t.starts_with("self.P_BLINDS = {") {
            section = "blinds";
            continue;
        }
        if t.starts_with("self.P_TAGS = {") {
            section = "tags";
            continue;
        }
        if section.is_empty() {
            continue;
        }
        if t == "}" || t.starts_with("self.") {
            section = "";
            continue;
        }
        let Some((key, rest)) = t.split_once('=') else { continue };
        let key = key.trim();
        let rest = rest.trim_start();
        if !rest.starts_with('{') || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let (v, _) = lua::parse_prefix(rest).map_err(|e| format!("game.lua line {}: {e}", lineno + 1))?;
        let s = |k: &str| v.get(k).str().unwrap_or_default().to_string();
        match section {
            "centers" => centers.push(Center {
                key: key.to_string(),
                name: s("name"),
                set: s("set"),
                order: v.get("order").int().unwrap_or(0),
                rarity: v.get("rarity").int().map(|r| r as u8),
                cost: v.get("cost").int().unwrap_or(0),
                config: v.get("config").to_json(),
                blueprint_compat: v.get("blueprint_compat").bool(),
                eternal_compat: v.get("eternal_compat").bool(),
                perishable_compat: v.get("perishable_compat").bool(),
                enhancement_gate: v.get("enhancement_gate").str().map(str::to_string),
                yes_pool_flag: v.get("yes_pool_flag").str().map(str::to_string),
                no_pool_flag: v.get("no_pool_flag").str().map(str::to_string),
            }),
            "tags" => tags.push(Tag {
                key: key.to_string(),
                name: s("name"),
                config: v.get("config").to_json(),
                min_ante: v.get("min_ante").int(),
            }),
            _ => blinds.push(Blind {
                key: key.to_string(),
                name: s("name"),
                mult: v.get("mult").num().unwrap_or(1.0),
                dollars: v.get("dollars").int().unwrap_or(0),
                boss: v.get("boss").to_json(),
                debuff: v.get("debuff").to_json(),
            }),
        }
    }
    centers.sort_by(|a, b| (a.set.as_str(), a.order).cmp(&(b.set.as_str(), b.order)));
    let mut d = GameData { game_version: game_version.to_string(), centers, blinds, tags, index: HashMap::new() };
    d.reindex();
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_data_has_all_jokers() {
        let d = GameData::bundled();
        assert_eq!(d.jokers().count(), 150);
        let jolly = d.center("j_jolly").unwrap();
        assert_eq!(jolly.config["t_mult"], 8);
        assert_eq!(d.center("j_duo").unwrap().config["Xmult"], 2);
        assert_eq!(d.blind("bl_big").unwrap().mult, 1.5);
    }

    #[test]
    fn extracts_one_line_entries() {
        let src = "    self.P_CENTERS = {\n        j_joker=  {order = 1, rarity = 1, cost = 2, name = \"Joker\", pos = {x=0,y=0}, set = \"Joker\", config = {mult = 4}},\n    }\n    self.P_BLINDS = {\n        bl_big = {name = 'Big Blind', dollars = 4, mult = 1.5, boss_colour = HEX('fff')},\n    }\n";
        let d = extract_from_game_lua(src, "test").unwrap();
        assert_eq!(d.center("j_joker").unwrap().config["mult"], 4);
        assert_eq!(d.blind("bl_big").unwrap().dollars, 4);
    }
}
