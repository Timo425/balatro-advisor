//! "As if bought": the run state with shop options you've ticked already bought, so the rest
//! of the advice (packs, other buys, the blinds) can be read with them. Pack picks, packs and
//! rerolls can't be planned (their contents are random or used up on the spot).

use crate::advise::Analysis;
use crate::data::GameData;
use crate::save::RunState;

/// The state with each ticked option (by its label in `a.options`) bought, and a line per
/// option saying what was applied or why it wasn't.
pub fn apply(run: &RunState, a: &Analysis, data: &GameData, picks: &[String]) -> (RunState, Vec<String>) {
    let mut r = run.clone();
    let mut notes = Vec::new();
    for label in picks {
        let Some(o) = a.options.iter().find(|o| &o.label == label) else {
            notes.push(format!("{label}: no longer on offer"));
            continue;
        };
        if (o.cost as f64) > r.dollars {
            notes.push(format!("{label}: can't afford it (${} left)", r.dollars));
            continue;
        }
        let key = o.key.clone().unwrap_or_default();
        match o.kind.as_str() {
            "voucher" => {
                r.dollars -= o.cost as f64;
                r.vouchers.push(key.clone());
                if let Some(sh) = r.shop.as_mut() {
                    sh.vouchers.retain(|v| v.key != key);
                }
                let modelled = apply_voucher(&mut r, &key, data);
                notes.push(if modelled { format!("{label} (−${})", o.cost) } else { format!("{label} (−${}; its effect is not modelled)", o.cost) });
            }
            "joker" if !label.starts_with("pick ") => {
                let Some(c) = a.shop.iter().find(|c| c.key == key) else {
                    notes.push(format!("{label}: not found in the shop"));
                    continue;
                };
                let Some(idx) = r.shop.as_ref().and_then(|sh| sh.jokers.iter().position(|j| j.key == key)) else {
                    notes.push(format!("{label}: not found in the shop"));
                    continue;
                };
                let card = r.shop.as_mut().map(|sh| sh.jokers.remove(idx)).expect("shop checked above");
                r.dollars -= o.cost as f64;
                // "replace NAME[, put it rightmost]", the swap the advice picked
                let sold = c.action.strip_prefix("replace ").map(|rest| (rest.trim_end_matches(", put it rightmost"), rest.ends_with("rightmost")));
                match sold.and_then(|(name, right)| r.jokers.iter().position(|j| data.name(&j.key) == name).map(|i| (i, right))) {
                    Some((i, right)) => {
                        let gone = r.jokers.remove(i);
                        r.dollars += gone.sell_value as f64;
                        if right { r.jokers.push(card) } else { r.jokers.insert(i, card) }
                        notes.push(format!("{label} (−${}, sold {} +${})", o.cost, data.name(&gone.key), gone.sell_value));
                    }
                    None => {
                        r.jokers.push(card);
                        notes.push(format!("{label} (−${})", o.cost));
                    }
                }
            }
            "tarot" | "planet" if !label.starts_with("pick ") && !label.ends_with("(you have it)") => {
                let Some(idx) = r.shop.as_ref().and_then(|sh| sh.other_cards.iter().position(|x| x.key == key)) else {
                    notes.push(format!("{label}: not found in the shop"));
                    continue;
                };
                if r.consumables.len() as i64 >= r.consumable_slots {
                    notes.push(format!("{label}: no free consumable slot"));
                    continue;
                }
                let card = r.shop.as_mut().map(|sh| sh.other_cards.remove(idx)).expect("shop checked above");
                r.dollars -= o.cost as f64;
                r.consumables.push(card);
                notes.push(format!("{label} (−${})", o.cost));
            }
            _ => notes.push(format!("{label}: can't be planned (only shop jokers, vouchers, tarots and planets)")),
        }
    }
    (r, notes)
}

/// What voucher `key` changes in the run (card.lua `Card:apply_to_run`), applied to `r`; false
/// when its effect isn't modelled. The one place voucher effects are listed: planning applies
/// them, and the shop values a voucher by what it changes (`advise.rs`).
pub fn apply_voucher(r: &mut RunState, key: &str, data: &GameData) -> bool {
    let extra = data.center(key).and_then(|c| c.config.get("extra")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    match key {
        "v_tarot_merchant" | "v_tarot_tycoon" => { r.shop_rates.tarot = 4.0 * extra; true }
        "v_planet_merchant" | "v_planet_tycoon" => { r.shop_rates.planet = 4.0 * extra; true }
        "v_magic_trick" | "v_illusion" => { r.shop_rates.playing_card = extra; true }
        "v_seed_money" => { r.interest_cap = r.interest_cap.max(50); true }
        "v_money_tree" => { r.interest_cap = r.interest_cap.max(100); true }
        "v_overstock_norm" | "v_overstock_plus" => { r.shop_rates.slots += 1; true }
        "v_reroll_surplus" | "v_reroll_glut" => { r.base_reroll_cost -= 2; true }
        "v_grabber" | "v_nacho_tong" => { r.round_hands += 1; true }
        "v_wasteful" | "v_recyclomancy" => { r.round_discards += 1; true }
        "v_paint_brush" | "v_palette" => { r.hand_size += 1; true }
        "v_crystal_ball" => { r.consumable_slots += 1; true }
        "v_antimatter" => { r.joker_slots += 1; true }
        _ => false,
    }
}
