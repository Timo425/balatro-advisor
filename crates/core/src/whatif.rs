//! What-if checks: the board, deck and hand after a plan someone names (sell these, add
//! that, turn this card into Glass), simulated against the blinds ahead next to the board
//! as it is. It plans nothing itself; it's the engine behind "what if I…" questions.

use serde::Serialize;

use crate::data::GameData;
use crate::engine::{BlindRules, Board, Joker};
use crate::model::{Card, Edition};
use crate::save::RunState;
use crate::sim::{self, RoundRules, RoundStart};

/// A plan, as changes to the current run.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// The whole joker list after the plan (overrides `sell` / `add`). Owned jokers keep
    /// their current values; a joker named twice is copied (Ankh, Invisible Joker).
    pub jokers: Option<Vec<String>>,
    pub sell: Vec<String>,
    pub add: Vec<String>,
    /// Cards changed in hand or deck: (as it is, as it becomes)
    pub set_cards: Vec<(Card, Card)>,
    pub add_cards: Vec<Card>,
    pub remove_cards: Vec<Card>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlindOdds {
    pub label: String,
    pub target: f64,
    /// Chance to beat it on score
    pub p_win: f64,
    /// With Mr. Bones' save, when he's on the board
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p_with_bones: Option<f64>,
    /// Mean round total as a share of the target
    pub reach: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub jokers: Vec<String>,
    pub notes: Vec<String>,
    pub now: Vec<BlindOdds>,
    pub plan: Vec<BlindOdds>,
}

/// A joker named by key (`red_card`, `j_red_card`) or name (`Red Card`), with optional
/// `:foil` / `:holo` / `:poly` / `:negative`.
fn joker_key(spec: &str, data: &GameData) -> Result<(String, Option<Edition>), String> {
    let mut parts = spec.split(':');
    let name = parts.next().unwrap_or_default().trim();
    let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '_', '-', '.'], "");
    let want = norm(name.strip_prefix("j_").unwrap_or(name));
    let key = data
        .centers
        .iter()
        .filter(|c| c.set == "Joker")
        .find(|c| norm(c.key.strip_prefix("j_").unwrap_or(&c.key)) == want || norm(&c.name) == want)
        .map(|c| c.key.clone())
        .ok_or_else(|| format!("unknown joker '{name}'"))?;
    let mut edition = None;
    for m in parts {
        edition = Some(match m.to_ascii_lowercase().as_str() {
            "foil" => Edition::Foil,
            "holo" => Edition::Holo,
            "poly" | "polychrome" => Edition::Polychrome,
            "negative" => Edition::Negative,
            other => return Err(format!("unknown joker edition '{other}'")),
        });
    }
    Ok((key, edition))
}

pub fn run(run: &RunState, data: &GameData, plan: &Plan, sims: usize, seed: u64) -> Result<Outcome, String> {
    let base = Board::from_run(run, data);
    let mut notes = Vec::new();

    // Jokers after the plan
    let owned: Vec<Joker> = base.jokers.clone();
    let from_owned_or_new = |key: &str, edition: Option<Edition>, used: &mut Vec<bool>| -> Result<Joker, String> {
        let mut j = match owned.iter().enumerate().find(|(i, j)| j.key == key && !used[*i]) {
            Some((i, j)) => {
                used[i] = true;
                j.clone()
            }
            // named again: a copy of the one you own, else a fresh one
            None => owned.iter().find(|j| j.key == key).cloned().map_or_else(|| Joker::from_key(key, data).ok_or(format!("no joker {key}")), Ok)?,
        };
        if edition.is_some() {
            j.edition = edition;
        }
        Ok(j)
    };
    let jokers: Vec<Joker> = if let Some(list) = &plan.jokers {
        let mut used = vec![false; owned.len()];
        list.iter().map(|s| joker_key(s, data).and_then(|(k, e)| from_owned_or_new(&k, e, &mut used))).collect::<Result<_, _>>()?
    } else {
        let mut js = owned.clone();
        for s in &plan.sell {
            let (k, _) = joker_key(s, data)?;
            let i = js.iter().position(|j| j.key == k).ok_or_else(|| format!("you don't have {}", data.name(&k)))?;
            if run.jokers.iter().any(|sj| sj.key == k && sj.eternal) {
                notes.push(format!("{} is eternal: the game won't let you sell it", data.name(&k)));
            }
            js.remove(i);
        }
        for s in &plan.add {
            let (k, e) = joker_key(s, data)?;
            let mut j = Joker::from_key(&k, data).ok_or(format!("no joker {k}"))?;
            j.edition = e;
            js.push(j);
        }
        js
    };
    let negatives = jokers.iter().filter(|j| j.edition == Some(Edition::Negative)).count() as i64;
    if jokers.len() as i64 > run.joker_slots + negatives {
        notes.push(format!("{} jokers but only {} slots", jokers.len(), run.joker_slots + negatives));
    }

    // Cards after the plan: changes land on the hand first, then the draw pile
    let mut hand = run.hand.clone();
    let mut pile = run.draw_pile.clone();
    let same = |a: &Card, b: &Card| a.same_kind(b);
    for (from, to) in &plan.set_cards {
        if let Some(c) = hand.iter_mut().find(|c| same(c, from)) {
            *c = *to;
        } else if let Some(c) = pile.iter_mut().find(|c| same(c, from)) {
            *c = *to;
        } else {
            return Err(format!("no card {} in your hand or draw pile", from.label()));
        }
    }
    for c in &plan.remove_cards {
        if let Some(i) = hand.iter().position(|x| same(x, c)) {
            hand.remove(i);
        } else if let Some(i) = pile.iter().position(|x| same(x, c)) {
            pile.remove(i);
        } else {
            return Err(format!("no card {} to remove", c.label()));
        }
    }
    pile.extend(plan.add_cards.iter().copied());
    let full_after: Vec<Card> = hand.iter().chain(pile.iter()).chain(run.discard_pile.iter()).copied().collect();

    let board_with = |js: &[Joker], deck: &[Card]| {
        let mut b = base.clone();
        b.jokers = js.to_vec();
        let tally = |e: crate::model::Enhancement| deck.iter().filter(|c| c.enhancement == Some(e)).count() as i64;
        b.steel_tally = tally(crate::model::Enhancement::Steel);
        b.stone_tally = tally(crate::model::Enhancement::Stone);
        b.driver_tally = deck.iter().filter(|c| c.enhancement.is_some()).count() as i64;
        b.playing_cards = deck.len() as i64;
        b
    };
    let odds = |js: &[Joker], hand: &[Card], pile: &[Card], full: &[Card]| -> Vec<BlindOdds> {
        let mut out = Vec::new();
        let upcoming = run.blinds.iter().filter(|bl| matches!(bl.state.as_str(), "Select" | "Upcoming" | "Current"));
        for bl in upcoming {
            let current = bl.state == "Current" && !hand.is_empty();
            let mut b = board_with(js, full);
            let rules = RoundRules::for_blind(&bl.key);
            let start = if current {
                let cb = run.current_blind.as_ref();
                b.deck_remaining = pile.len() as i64;
                RoundStart {
                    hand: hand.to_vec(),
                    deck: pile.to_vec(),
                    hand_size: run.hand_size,
                    hands: run.hands_left,
                    discards: run.discards_left,
                    scored: cb.map_or(0.0, |c| c.scored),
                    target: cb.map_or(bl.target, |c| c.target),
                }
            } else {
                b.blind = BlindRules { key: bl.key.clone(), ..Default::default() };
                let mut deck = full.to_vec();
                let f = b.rule_flags();
                rules.apply(&mut deck, f.smeared, f.pareidolia);
                RoundStart {
                    hand: vec![],
                    deck,
                    hand_size: run.hand_size + rules.hand_size_delta,
                    hands: if bl.key == "bl_needle" { 1 } else { run.round_hands },
                    discards: if bl.key == "bl_water" { 0 } else { run.round_discards },
                    scored: 0.0,
                    target: bl.target,
                }
            };
            let (p, st) = sim::round_odds(&b, &start, sims, seed);
            let bones = js.iter().any(|j| j.key == "j_mr_bones" && !j.debuff);
            let name = if bl.slot == "Boss" { bl.name.clone() } else { format!("{} Blind", bl.slot) };
            out.push(BlindOdds {
                label: if current { format!("{name} (in progress)") } else { name },
                target: start.target,
                p_win: p,
                p_with_bones: bones.then_some(st.p_saved),
                reach: st.mean / start.target.max(1.0),
            });
        }
        // Next ante: a plain boss, to see how far the board carries
        let ante = run.ante + 1;
        let target = crate::save::blind_amount(ante, run.blind_scaling) * 2.0 * run.ante_scaling;
        let b = board_with(js, full);
        let start = RoundStart { hand: vec![], deck: full.to_vec(), hand_size: run.hand_size, hands: run.round_hands, discards: run.round_discards, scored: 0.0, target };
        let (p, st) = sim::round_odds(&b, &start, sims, seed);
        out.push(BlindOdds { label: format!("Ante {ante} plain boss"), target, p_win: p, p_with_bones: None, reach: st.mean / target.max(1.0) });
        out
    };
    let full_now = run.full_deck();
    Ok(Outcome {
        jokers: jokers.iter().map(|j| format!("{}{}", data.name(&j.key), j.edition.map_or(String::new(), |e| format!(" ({e:?})").to_lowercase()))).collect(),
        notes,
        now: odds(&base.jokers, &run.hand, &run.draw_pile, &full_now),
        plan: odds(&jokers, &hand, &pile, &full_after),
    })
}
