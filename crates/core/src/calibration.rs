//! Calibration log: what the advisor predicted at the start of each blind, and what
//! happened. Over many blinds, predictions of "70%" should win about 70% of the time;
//! that is the real test of the round simulation. Also records the final board of every
//! won run, as evidence for what winning boards look like, and every shop (and opened
//! pack) seen with the board and money at the time: consecutive shop entries show what
//! was bought, sold and skipped.
//!
//! Stored as JSON lines in `~/.local/share/balatro-advisor/` (never in the repo).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::save::RunState;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    /// Win chance at the start of a blind (first hand, nothing scored yet).
    Prediction { id: String, seed: String, ante: i64, slot: String, blind: String, target: f64, p_win: f64, stake: u8, time: u64 },
    /// How that blind ended.
    Outcome { id: String, won: bool, time: u64 },
    /// A won run's final board.
    Win { seed: String, deck: String, stake: u8, jokers: Vec<String>, hand_levels: Vec<(String, i64)>, time: u64 },
    /// A shop or an opened pack as seen (logged again whenever what's on offer, the board
    /// or the money changes).
    Shop {
        seed: String,
        ante: i64,
        round: i64,
        screen: String,
        dollars: f64,
        jokers: Vec<String>,
        consumables: Vec<String>,
        offers: Vec<String>,
        pack: Vec<String>,
        time: u64,
    },
}

pub fn default_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("balatro-advisor"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn read(dir: &Path) -> Vec<Entry> {
    std::fs::read_to_string(dir.join("calibration.jsonl"))
        .map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

/// Watches successive run states and appends predictions and outcomes.
pub struct Tracker {
    dir: PathBuf,
    /// Predictions without an outcome yet (restored from the file on start).
    open: Vec<(String, String, i64, String)>, // id, seed, ante, slot
    wins_logged: Vec<String>,
    /// The last shop entry, to log only changes
    last_shop: Option<Entry>,
}

impl Tracker {
    pub fn new(dir: PathBuf) -> Tracker {
        let entries = read(&dir);
        let mut open = Vec::new();
        let mut wins_logged = Vec::new();
        for e in &entries {
            match e {
                Entry::Prediction { id, seed, ante, slot, .. } => open.push((id.clone(), seed.clone(), *ante, slot.clone())),
                Entry::Outcome { id, .. } => open.retain(|o| &o.0 != id),
                Entry::Win { seed, .. } => wins_logged.push(seed.clone()),
                Entry::Shop { .. } => {}
            }
        }
        Tracker { dir, open, wins_logged, last_shop: None }
    }

    fn append(&self, e: &Entry) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("calibration.jsonl")) {
            let _ = writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default());
        }
    }

    /// Feed every new run state. `p_current`: the advisor's win chance for the blind in
    /// progress, if any. `None` for `run` means no run (e.g. it just ended).
    pub fn observe(&mut self, run: Option<&RunState>, p_current: Option<f64>) -> Vec<Entry> {
        let mut out = Vec::new();
        let seed = run.map(|r| r.seed.clone()).unwrap_or_default();
        // Close predictions whose blind is decided
        let mut still = Vec::new();
        for (id, s, ante, slot) in std::mem::take(&mut self.open) {
            let decided = match run {
                Some(r) if r.seed == s => {
                    let later_ante = r.ante > ante;
                    let this = r.blinds.iter().find(|b| b.slot == slot);
                    let beaten = r.ante == ante && this.is_some_and(|b| b.state == "Defeated");
                    if beaten || later_ante || r.won {
                        Some(true)
                    } else if r.screen == crate::save::Screen::GameOver {
                        Some(false)
                    } else {
                        None
                    }
                }
                // A different run, or none: the old run ended in that blind (a win would
                // have been seen as "Defeated" first).
                _ => Some(false),
            };
            match decided {
                Some(won) => out.push(Entry::Outcome { id, won, time: now() }),
                None => still.push((id, s, ante, slot)),
            }
        }
        self.open = still;

        if let Some(r) = run {
            // A blind that has just started: first hand, nothing scored
            if let (true, Some(cb), Some(p)) = (r.screen.in_blind(), &r.current_blind, p_current) {
                let slot = r.blinds.iter().find(|b| b.state == "Current").map(|b| b.slot.clone()).unwrap_or_default();
                let fresh = cb.scored == 0.0 && r.hands_left == r.round_hands;
                let id = format!("{}-{}-{}", r.seed, r.ante, slot);
                let known = self.open.iter().any(|o| o.0 == id) || read(&self.dir).iter().any(|e| matches!(e, Entry::Prediction { id: i, .. } if *i == id));
                if fresh && !slot.is_empty() && !known {
                    out.push(Entry::Prediction {
                        id: id.clone(),
                        seed: r.seed.clone(),
                        ante: r.ante,
                        slot: slot.clone(),
                        blind: cb.name.clone(),
                        target: cb.target,
                        p_win: p,
                        stake: r.stake,
                        time: now(),
                    });
                    self.open.push((id, r.seed.clone(), r.ante, slot));
                }
            }
            if let Some(e) = shop_entry(r) {
                let same = |a: &Entry, b: &Entry| match (a, b) {
                    (Entry::Shop { .. }, Entry::Shop { .. }) => {
                        let strip = |e: &Entry| match e {
                            Entry::Shop { seed, ante, round, screen, dollars, jokers, consumables, offers, pack, .. } => {
                                (seed.clone(), *ante, *round, screen.clone(), *dollars, jokers.clone(), consumables.clone(), offers.clone(), pack.clone())
                            }
                            _ => unreachable!(),
                        };
                        strip(a) == strip(b)
                    }
                    _ => false,
                };
                if self.last_shop.as_ref().is_none_or(|l| !same(l, &e)) {
                    self.last_shop = Some(e.clone());
                    out.push(e);
                }
            }
            if r.won && !self.wins_logged.contains(&seed) {
                out.push(Entry::Win {
                    seed: seed.clone(),
                    deck: r.deck.clone(),
                    stake: r.stake,
                    jokers: r.jokers.iter().map(|j| j.name.clone()).collect(),
                    hand_levels: r.hand_levels.iter().filter(|(_, h)| h.level > 1).map(|(n, h)| (n.clone(), h.level)).collect(),
                    time: now(),
                });
                self.wins_logged.push(seed);
            }
        }
        for e in &out {
            self.append(e);
        }
        out
    }
}

/// The shop (or opened pack) on screen, as a log entry.
fn shop_entry(r: &RunState) -> Option<Entry> {
    let in_shop = matches!(r.screen, crate::save::Screen::Shop) || r.screen.in_pack();
    if !in_shop {
        return None;
    }
    let joker = |j: &crate::save::JokerCard| {
        let mut tags: Vec<String> = Vec::new();
        if let Some(e) = j.edition {
            tags.push(format!("{e:?}").to_lowercase());
        }
        if j.eternal {
            tags.push("eternal".into());
        }
        if let Some(p) = j.perishable {
            tags.push(format!("perishable {p}"));
        }
        if j.rental {
            tags.push("rental".into());
        }
        if tags.is_empty() { j.name.clone() } else { format!("{} ({})", j.name, tags.join(", ")) }
    };
    let item = |c: &crate::save::ItemCard| c.card.map_or_else(|| c.name.clone(), |card| card.label());
    let mut offers = Vec::new();
    if let Some(sh) = &r.shop {
        offers.extend(sh.jokers.iter().map(|j| format!("{} ${}", joker(j), j.cost)));
        offers.extend(sh.other_cards.iter().chain(&sh.boosters).chain(&sh.vouchers).map(|c| format!("{} ${}", item(c), c.cost)));
    }
    Some(Entry::Shop {
        seed: r.seed.clone(),
        ante: r.ante,
        round: r.round,
        screen: format!("{:?}", r.screen),
        dollars: r.dollars,
        jokers: r.jokers.iter().map(joker).collect(),
        consumables: r.consumables.iter().map(|c| c.name.clone()).collect(),
        offers,
        pack: r.open_pack.iter().map(item).collect(),
        time: now(),
    })
}

/// "Predicted 70–85%: won 11 of 14 (79%)" rows.
#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    pub from: f64,
    pub to: f64,
    pub n: usize,
    pub predicted: f64,
    pub actual: f64,
}

pub fn report(entries: &[Entry]) -> Vec<Bucket> {
    let outcome = |id: &str| {
        entries.iter().find_map(|e| match e {
            Entry::Outcome { id: i, won, .. } if i == id => Some(*won),
            _ => None,
        })
    };
    let pairs: Vec<(f64, bool)> = entries
        .iter()
        .filter_map(|e| match e {
            Entry::Prediction { id, p_win, .. } => outcome(id).map(|w| (*p_win, w)),
            _ => None,
        })
        .collect();
    let edges = [0.0, 0.5, 0.7, 0.85, 0.95, 1.0001];
    edges
        .windows(2)
        .map(|w| {
            let inb: Vec<&(f64, bool)> = pairs.iter().filter(|(p, _)| *p >= w[0] && *p < w[1]).collect();
            let n = inb.len();
            Bucket {
                from: w[0],
                to: w[1].min(1.0),
                n,
                predicted: if n > 0 { inb.iter().map(|(p, _)| p).sum::<f64>() / n as f64 } else { 0.0 },
                actual: if n > 0 { inb.iter().filter(|(_, w)| *w).count() as f64 / n as f64 } else { 0.0 },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::{BlindSlot, CurrentBlind, Screen};

    fn run_at(state: &str, screen: Screen, scored: f64) -> RunState {
        let mut r: RunState = serde_json::from_value(serde_json::json!({
            "seed": "ABC", "won": false, "game_version": "", "screen": "selecting_hand", "stake": 8, "deck": "Red Deck",
            "ante": 2, "win_ante": 8, "blind_scaling": 3, "ante_scaling": 1.0, "round": 4, "dollars": 10.0,
            "interest_amount": 1, "interest_cap": 25, "money_per_hand": 1.0, "base_reroll_cost": 5, "skips": 0, "hands_played": 0,
            "tarots_used": 0, "starting_deck_size": 52, "most_played_hand": "", "hands_left": 4, "discards_left": 3,
            "round_hands": 4, "round_discards": 3, "hand_size": 8, "joker_slots": 5, "consumable_slots": 2, "probability_normal": 1.0,
            "jokers": [], "consumables": [], "hand": [], "draw_pile": [], "discard_pile": [], "hand_levels": {}, "blinds": [],
            "current_blind": null, "shop": null, "open_pack": [], "vouchers": [], "tags": [],
            "round_targets": {"ancient_suit": null, "castle_suit": null, "idol": null, "mail_rank": null},
            "used_jokers": [], "pool_flags": [], "banned_keys": [],
            "shop_rates": {"joker": 20.0, "tarot": 4.0, "planet": 4.0, "spectral": 0.0, "playing_card": 0.0, "slots": 2},
            "snapshot": {"path": "x", "live": false, "age_secs": null, "caveats": []}
        }))
        .unwrap();
        r.screen = screen;
        r.blinds = vec![BlindSlot { slot: "Big".into(), key: "bl_big".into(), name: "Big Blind".into(), state: state.into(), target: 1500.0, reward: 4, skip_tag: None }];
        r.current_blind = Some(CurrentBlind { key: "bl_big".into(), name: "Big Blind".into(), target: 1500.0, scored, disabled: false, hands_seen: vec![], only_hand: None });
        r
    }

    #[test]
    fn records_a_prediction_then_its_outcome() {
        let dir = std::env::temp_dir().join(format!("bav-cal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = Tracker::new(dir.clone());
        let e = t.observe(Some(&run_at("Current", Screen::SelectingHand, 0.0)), Some(0.8));
        assert!(matches!(e.as_slice(), [Entry::Prediction { p_win, .. }] if *p_win == 0.8));
        // mid-blind: no new prediction, no outcome
        assert!(t.observe(Some(&run_at("Current", Screen::SelectingHand, 500.0)), Some(0.9)).is_empty());
        // beaten
        let e = t.observe(Some(&run_at("Defeated", Screen::Shop, 0.0)), None);
        assert!(matches!(e.as_slice(), [Entry::Outcome { won: true, .. }, Entry::Shop { .. }]), "{e:?}");
        // the same shop again isn't logged twice
        assert!(t.observe(Some(&run_at("Defeated", Screen::Shop, 0.0)), None).is_empty());
        // a restart doesn't predict the same blind twice
        let mut t2 = Tracker::new(dir.clone());
        assert!(t2.observe(Some(&run_at("Current", Screen::SelectingHand, 0.0)), Some(0.8)).is_empty());
        let b = report(&read(&dir));
        assert_eq!(b.iter().map(|b| b.n).sum::<usize>(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_run_that_disappears_mid_blind_is_a_loss() {
        let dir = std::env::temp_dir().join(format!("bav-cal2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = Tracker::new(dir.clone());
        t.observe(Some(&run_at("Current", Screen::SelectingHand, 0.0)), Some(0.4));
        let e = t.observe(None, None);
        assert!(matches!(e.as_slice(), [Entry::Outcome { won: false, .. }]));
        std::fs::remove_dir_all(dir).ok();
    }
}
