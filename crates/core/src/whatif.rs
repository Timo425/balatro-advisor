//! What-if checks: the board, deck and hand after a plan someone names (sell these, add
//! that, turn this card into Glass), simulated against the blinds ahead next to the board
//! as it is. It plans nothing itself; it's the engine behind "what if I…" questions.

use serde::Serialize;

use crate::data::GameData;
use crate::engine::{Board, Joker};
use crate::model::{Card, Edition};
use crate::save::RunState;
use crate::sim::{self, RoundStart};

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
    /// Money the round pays for how it's played, on average over every round (Lucky cards, Gold
    /// Seals, discards that pay, the money jokers, Gold cards held at its end, a hand left at the
    /// cash-out; a round lost pays what it paid while played): the simulated round's own
    /// (`sim::round_odds`). `Money::per_round` is over the rounds won.
    pub round_money: f64,
}

/// What the plan does to your money, beside the score: the odds above count only what the
/// jokers score, so a joker that pays (Rocket, Golden Joker) looks like an empty slot there.
#[derive(Debug, Clone, Serialize)]
pub struct Money {
    pub dollars: f64,
    /// After the plan: what selling gives back (`sell_value`), less what buying costs (the
    /// shop's price, else the joker's base price)
    pub dollars_after: f64,
    /// What a round won pays besides its reward and interest, less rent ($3 a rental): now (the
    /// mean over the rounds won of this ante's three plain blinds: scoring, discards, the round's
    /// end, hands left) and with the plan (that, and the difference on the same rounds both win,
    /// past the draws: `value::money_difference`, the advice's measure), plus what the jokers pay
    /// besides a round's play at this round's rate (Egg's sell value, To the Moon's interest;
    /// Rocket's later raises not counted)
    pub per_round: f64,
    pub per_round_after: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub jokers: Vec<String>,
    pub notes: Vec<String>,
    pub now: Vec<BlindOdds>,
    pub plan: Vec<BlindOdds>,
    pub money: Money,
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
    // The jokers the plan adds and takes away (by key; an owned one kept is neither): their hand
    // size, hands and discards change every round not yet started (`advise::apply_mods`)
    let (added, removed): (Vec<&Joker>, Vec<&Joker>) = {
        let mut left: Vec<&Joker> = jokers.iter().collect();
        let mut removed = vec![];
        for o in &owned {
            match left.iter().position(|j| j.key == o.key) {
                Some(i) => {
                    left.remove(i);
                }
                None => removed.push(o),
            }
        }
        (left, removed)
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
        b.count_deck(deck);
        b
    };
    let odds = |js: &[Joker], hand: &[Card], pile: &[Card], full: &[Card], added: &[&Joker], removed: &[&Joker]| -> Vec<BlindOdds> {
        let mut out = Vec::new();
        let upcoming = run.blinds.iter().filter(|bl| matches!(bl.state.as_str(), "Select" | "Upcoming" | "Current"));
        for (i, bl) in upcoming.enumerate() {
            // the blind in play, its first cards dealt or not (DrawToHand): its blind was set
            // when it started, as advise's round in progress
            let current = bl.state == "Current" && run.blind_in_play().is_some();
            let mut b = board_with(js, full);
            let start = if current {
                let cb = run.blind_in_play();
                b.deck_remaining = pile.len() as i64;
                RoundStart {
                    hand: hand.to_vec(),
                    deck: pile.to_vec(),
                    hand_size: run.round_hand_size(),
                    hands: run.hands_left,
                    discards: run.discards_left,
                    scored: cb.map_or(0.0, |c| c.scored),
                    target: cb.map_or(bl.target, |c| c.target),
                    fresh: false,
                    blind: bl.key.clone(),
                    disabled: cb.is_some_and(|c| c.disabled),
                    juggle: 0,
                }
            } else {
                let start = RoundStart {
                    hand: vec![],
                    deck: full.to_vec(),
                    hand_size: run.hand_size,
                    hands: run.round_hands,
                    discards: run.round_discards,
                    scored: 0.0,
                    target: bl.target,
                    fresh: true,
                    blind: bl.key.clone(),
                    disabled: false,
                    // the next round played draws with the Juggle Tags held
                    juggle: if i == 0 { run.round_start_tags() } else { 0 },
                };
                // the plan's jokers; all of blind.lua Blind:set_blind, then the jokers',
                // come as it's set on the board, per round (sim::round_odds -> Board::at_blind_set_drawn)
                crate::advise::apply_mods(&start, added, removed)
            };
            let (p, st) = sim::round_odds(&b, &start, sims, seed);
            let bones = b.round_saves() > 0;
            let name = if bl.slot == "Boss" { bl.name.clone() } else { format!("{} Blind", bl.slot) };
            out.push(BlindOdds {
                label: if current { format!("{name} (in progress)") } else { name },
                target: start.target,
                p_win: p,
                p_with_bones: bones.then_some(st.p_saved),
                reach: st.mean / start.target.max(1.0),
                round_money: st.money + st.hands_left * run.money_per_hand,
            });
        }
        // Next ante: a plain boss, to see how far the board carries
        let ante = run.ante + 1;
        let target = crate::save::blind_amount(ante, run.blind_scaling) * 2.0 * run.ante_scaling;
        let b = board_with(js, full);
        let start = RoundStart { hand: vec![], deck: full.to_vec(), hand_size: run.hand_size, hands: run.round_hands, discards: run.round_discards, scored: 0.0, target, fresh: true, ..Default::default() };
        let start = crate::advise::apply_mods(&start, added, removed);
        let (p, st) = sim::round_odds(&b, &start, sims, seed);
        out.push(BlindOdds { label: format!("Ante {ante} plain boss"), target, p_win: p, p_with_bones: None, reach: st.mean / target.max(1.0), round_money: st.money + st.hands_left * run.money_per_hand });
        out
    };
    // Money: the jokers sold and bought (by key, as above), and the payouts and rent of the
    // board you'd hold. Owned jokers are matched to the save's (their own counters, rental);
    // one bought comes from the shop when it's there, else fresh at its base price.
    let mut shop: Vec<&crate::save::JokerCard> = run.shop.as_ref().map(|s| s.jokers.iter().collect()).unwrap_or_default();
    let mut owned_left: Vec<&crate::save::JokerCard> = run.jokers.iter().collect();
    let sold: i64 = removed.iter().filter_map(|j| owned_left.iter().position(|sj| sj.key == j.key).map(|i| owned_left.remove(i).sell_value)).sum();
    let fresh = |key: &str| data.center(key).map(|c| (crate::engine::joker::ability_from_config(&c.config), c.cost));
    let mut bought = 0;
    let mut rentals = 0usize;
    for j in &jokers {
        if let Some(i) = owned_left.iter().position(|sj| sj.key == j.key) {
            rentals += owned_left.remove(i).rental as usize;
        } else if let Some(i) = shop.iter().position(|sj| sj.key == j.key) {
            let sj = shop.remove(i);
            bought += sj.cost;
            rentals += sj.rental as usize;
        } else {
            bought += fresh(&j.key).map_or(0, |c| c.1);
        }
    }
    // on this ante's plain blinds, the same rounds now and with the plan, without the current
    // blind's debuffs (a perished joker's stay: it scores and pays nothing), as
    // `advise::plain_rounds_of`
    let perished = |js: &mut [Joker]| {
        for j in js.iter_mut() {
            j.debuff = run.jokers.iter().any(|sj| sj.key == j.key && sj.perishable == Some(0));
        }
    };
    let n = sims.div_ceil(3).max(1);
    // each round's cash and what the jokers pay besides (`value::payout_besides` at this round's
    // rate), less rent
    let rounds = |js: &[Joker], deck: &[Card], added: &[&Joker], removed: &[&Joker], rentals: usize| -> (Vec<Option<f64>>, f64) {
        let mut js = js.to_vec();
        perished(&mut js);
        let starts: Vec<RoundStart> = crate::advise::plain_rounds(run, deck).iter().map(|s| crate::advise::apply_mods(s, added, removed)).collect();
        let b = board_with(&js, deck);
        let besides: f64 = js.iter().map(|j| crate::advise::payout_besides(j, &b, run, 0.0)).sum();
        (crate::advise::plain_round_cash(&b, &starts, n, seed, (run.money_per_hand, run.money_per_discard)), besides - rentals as f64 * 3.0)
    };
    let full_now = run.full_deck();
    let (now, now_besides) = rounds(&base.jokers, &full_now, &[], &[], run.jokers.iter().filter(|j| j.rental).count());
    let (after, after_besides) = rounds(&jokers, &full_after, &added, &removed, rentals);
    // now: the mean over the rounds won; with the plan: that, and the difference on the rounds
    // both win (`value::money_difference`, the advice's own measure)
    let now_money = crate::advise::won_mean(&now);
    let money = Money {
        dollars: run.dollars,
        dollars_after: run.dollars + (sold - bought) as f64,
        per_round: now_money + now_besides,
        per_round_after: now_money + crate::advise::money_difference(&after, &now) + after_besides,
    };
    if money.dollars_after < 0.0 {
        notes.push(format!("the plan costs ${:.0} more than the ${:.0} you have", -money.dollars_after, run.dollars));
    }

    Ok(Outcome {
        jokers: jokers.iter().map(|j| format!("{}{}", data.name(&j.key), j.edition.map_or(String::new(), |e| format!(" ({e:?})").to_lowercase()))).collect(),
        notes,
        now: odds(&base.jokers, &run.hand, &run.draw_pile, &full_now, &[], &[]),
        plan: odds(&jokers, &hand, &pile, &full_after, &added, &removed),
        money,
    })
}
