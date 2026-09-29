//! The advice: what each joker is worth, the best order, which shop joker helps,
//! and which jokers would rescue the run.
//!
//! Everything is measured on the same simulated deals (common random numbers), so the
//! differences between boards are much steadier than the raw numbers.

use std::time::Instant;

use serde::Serialize;

use crate::data::{GameData, RARITY_NAMES};
use crate::engine::{Board, Joker, Kind};
use crate::model::Card;
use crate::gold::GoldReport;
use crate::model::Edition;
use crate::save::RunState;
use crate::sim::{self, RoundRules, RoundStart, Stats};

#[derive(Debug, Clone)]
pub struct Options {
    /// Round simulations per board for the blind odds.
    pub sims: usize,
    /// Fewer simulations for the first pass over every possible joker.
    pub screen_sims: usize,
    /// Fresh deals for the "typical best hand" number.
    pub hand_samples: usize,
    pub seed: u64,
    /// How many rescue candidates to report.
    pub rescue_top: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { sims: 300, screen_sims: 60, hand_samples: 300, seed: 42, rescue_top: 12 }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Round {
    pub label: String,
    pub blind: String,
    pub target: f64,
    /// Chance to beat it with the current jokers (heuristic play/discard policy).
    pub p_win: f64,
    pub total: Stats,
    pub in_progress: bool,
    /// A plain boss in a later ante: "how far does this board carry".
    pub horizon: bool,
    /// Mean round total as a share of the target.
    pub reach: f64,
    /// Boss effects this simulation does not model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unmodelled: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JokerReport {
    pub key: String,
    pub name: String,
    pub rarity: String,
    pub edition: Option<Edition>,
    pub roles: Vec<&'static str>,
    /// Typical best hand drops by this share when the joker is removed (0.25 = 25%).
    pub score_share: f64,
    /// Change in the chance to beat each round if the joker were sold.
    pub p_win_if_removed: Vec<f64>,
    pub eternal: bool,
    pub debuff: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderAdvice {
    /// Joker names left to right.
    pub order: Vec<String>,
    /// Typical best hand gain vs the current order (0.1 = +10%).
    pub gain: f64,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub key: String,
    pub name: String,
    pub rarity: String,
    pub cost: i64,
    pub edition: Option<Edition>,
    /// `add`, or `replace <name>` when slots are full.
    pub action: String,
    pub p_win: Vec<f64>,
    pub p_win_delta: Vec<f64>,
    /// Per round: mean round total as a share of the target (useful where the win
    /// chance is ~0, e.g. the next ante), and the change from adding this joker.
    pub reach: Vec<f64>,
    pub reach_delta: Vec<f64>,
    /// Typical best hand change (0.3 = +30%).
    pub score_gain: f64,
    pub missing_gold: bool,
    /// 1 Common, 2 Uncommon, 3 Rare.
    pub rarity_n: u8,
    /// Odds from the full number of simulations (the top of the ranking) rather than the quick screen.
    pub precise: bool,
    /// Chance a given shop shows it (all card slots, before rerolls).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_shop: Option<f64>,
    pub roles: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayAdvice {
    pub cards: Vec<String>,
    pub hand: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub schema_version: u32,
    pub deck: String,
    pub stake: u8,
    pub ante: i64,
    pub screen: String,
    pub dollars: f64,
    pub joker_slots: i64,
    pub rounds: Vec<Round>,
    pub typical_hand: Stats,
    /// Which hands the board ends up playing, best hand per fresh deal.
    pub hand_mix: Vec<HandShare>,
    pub jokers: Vec<JokerReport>,
    pub order: Option<OrderAdvice>,
    pub shop: Vec<Candidate>,
    pub rescue: Vec<Candidate>,
    pub blinds: Vec<BlindView>,
    /// Every joker the shop can still offer. The top ones carry full-precision odds,
    /// the rest screening-quality ones (fewer simulations).
    pub pool: Vec<Candidate>,
    pub shop_odds: ShopOdds,
    pub best_play: Option<PlayAdvice>,
    pub gold: Option<GoldSummary>,
    pub caveats: Vec<String>,
    pub heuristics: Vec<&'static str>,
    pub save_age_secs: Option<u64>,
    /// Read from the live mod rather than the checkpoint save.
    pub live: bool,
    pub elapsed_ms: u128,
}

/// One of this ante's three blinds.
#[derive(Debug, Clone, Serialize)]
pub struct BlindView {
    pub slot: String,
    pub name: String,
    pub state: String,
    pub target: f64,
    /// Chance to beat it now (None once defeated/skipped).
    pub p_win: Option<f64>,
    /// Cash for beating it, before unused-hand money and interest.
    pub reward: i64,
    pub skip_tag: Option<TagView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagView {
    pub key: String,
    pub name: String,
}


/// What the page needs to turn pool odds into "chance the next shop shows one".
#[derive(Debug, Clone, Serialize)]
pub struct ShopOdds {
    /// Chance a shop card slot is a joker.
    pub joker_share: f64,
    pub slots: i64,
    pub reroll_cost: i64,
    /// Jokers still in the pool per rarity (index 1..=3).
    pub pool_by_rarity: [usize; 4],
    pub rarity_weight: [f64; 4],
    pub money_per_hand: f64,
    pub interest_amount: i64,
    pub interest_cap: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandShare {
    pub hand: String,
    /// Share of the points scored in simulated rounds.
    pub share: f64,
    /// Share of the hands played.
    pub played: f64,
    pub mean: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GoldSummary {
    pub have: usize,
    pub total: usize,
    pub this_run_counts: bool,
}

/// Roles for display. Several can apply.
pub fn roles(j: &Joker) -> Vec<&'static str> {
    use Kind::*;
    let mut r = Vec::new();
    let scoring = j.kind != Other || j.x_mult > 1.0 || j.t_mult > 0.0 || j.t_chips > 0.0 || scaling_key(&j.key);
    if matches!(j.kind, FourFingers | Shortcut | Smeared | Splash | Pareidolia) {
        r.push("rule");
    } else if matches!(j.kind, SockAndBuskin | HangingChad | Dusk | Seltzer | Hack | Mime) {
        r.push("retrigger");
    } else if scoring {
        r.push("scoring");
    }
    if scaling_key(&j.key) {
        r.push("scaling");
    }
    if economy_key(&j.key) {
        r.push("economy");
    }
    if j.key == "j_mr_bones" {
        r.push("survival");
    }
    if r.is_empty() {
        r.push("utility");
    }
    r
}

fn scaling_key(k: &str) -> bool {
    matches!(
        k,
        "j_green_joker" | "j_ride_the_bus" | "j_trousers" | "j_square" | "j_runner" | "j_wee" | "j_castle"
            | "j_lucky_cat" | "j_vampire" | "j_obelisk" | "j_hologram" | "j_constellation" | "j_madness"
            | "j_glass" | "j_campfire" | "j_hit_the_road" | "j_ceremonial" | "j_flash" | "j_red_card"
            | "j_throwback" | "j_hiker" | "j_caino" | "j_yorick" | "j_ramen" | "j_ice_cream" | "j_popcorn"
            | "j_supernova" | "j_fortune_teller"
    )
}

fn economy_key(k: &str) -> bool {
    matches!(
        k,
        "j_credit_card" | "j_delayed_grat" | "j_egg" | "j_business" | "j_faceless" | "j_todo_list" | "j_golden"
            | "j_rocket" | "j_gift" | "j_reserved_parking" | "j_mail" | "j_to_the_moon" | "j_satellite"
            | "j_cloud_9" | "j_trading" | "j_ticket" | "j_rough_gem" | "j_matador"
    )
}

/// A short label for jokers whose value isn't a score number.
fn non_scoring_note(key: &str) -> Option<&'static str> {
    Some(match key {
        "j_mr_bones" => "Survival: saves a lost round if you scored at least 25% of the blind (heuristic, not simulated)",
        "j_juggler" | "j_troubadour" | "j_turtle_bean" | "j_merry_andy" | "j_drunkard" | "j_burglar" => {
            "Changes hand size / hands / discards: simulated"
        }
        "j_golden" | "j_rocket" | "j_cloud_9" | "j_delayed_grat" | "j_to_the_moon" | "j_satellite" => {
            "Economy: money per round, not a score (heuristic)"
        }
        "j_egg" | "j_gift" => "Economy: grows sell value (heuristic)",
        "j_credit_card" => "Economy: lets you go to -$20",
        "j_chaos" => "Utility: 1 free reroll per shop",
        "j_ring_master" => "Utility: duplicate jokers can appear",
        "j_oops" => "Doubles every probability: simulated",
        "j_dna" | "j_marble" | "j_sixth_sense" | "j_certificate" | "j_vampire" | "j_midas_mask" => {
            "Deck-fixing: changes your cards over time (only this hand's effect is simulated)"
        }
        "j_8_ball" | "j_superposition" | "j_seance" | "j_riff_raff" | "j_vagabond" | "j_hallucination"
        | "j_cartomancer" | "j_astronomer" | "j_burnt" | "j_perkeo" | "j_invisible" | "j_diet_cola" => {
            "Utility: makes cards/jokers, value not simulated"
        }
        "j_luchador" | "j_chicot" => "Utility: disables boss effects (not simulated)",
        _ => return None,
    })
}

/// Hand size / hands / discards a joker adds while owned (its `add_to_deck` effects).
fn round_mods(j: &Joker) -> (i64, i64, i64) {
    match j.key.as_str() {
        "j_juggler" => (1, 0, 0),
        "j_troubadour" => (2, -1, 0),
        "j_turtle_bean" => (j.extra_h_size() as i64, 0, 0),
        "j_stuntman" => (-2, 0, 0),
        "j_merry_andy" => (-1, 0, 3),
        "j_drunkard" => (0, 0, 1),
        "j_burglar" => (0, 3, -99),
        _ => (0, 0, 0),
    }
}

impl Joker {
    fn extra_h_size(&self) -> f64 {
        if self.key == "j_turtle_bean" { 5.0 } else { 0.0 }
    }
}

/// A round to simulate, before joker-specific adjustments.
#[derive(Clone)]
struct Spec {
    label: String,
    blind_key: String,
    blind_name: String,
    start: RoundStart,
    rules: RoundRules,
    in_progress: bool,
    horizon: bool,
}

fn apply_mods(start: &RoundStart, added: &[&Joker], removed: &[&Joker], fresh: bool) -> RoundStart {
    let mut s = start.clone();
    if !fresh {
        return s;
    }
    for (j, sign) in added.iter().map(|j| (j, 1)).chain(removed.iter().map(|j| (j, -1))) {
        let (h, hands, d) = round_mods(j);
        s.hand_size += sign * h;
        s.hands += sign * hands;
        if d == -99 {
            if sign > 0 {
                s.discards = 0;
            }
        } else {
            s.discards += sign * d;
        }
    }
    s.hand_size = s.hand_size.max(1);
    s.hands = s.hands.max(1);
    s.discards = s.discards.max(0);
    s
}

struct Ctx<'a> {
    run: &'a RunState,
    data: &'a GameData,
    base: Board,
    specs: Vec<Spec>,
    fresh_deck: Vec<Card>,
    opts: Options,
}

impl Ctx<'_> {
    fn board_for(&self, b: &Board, spec: &Spec) -> Board {
        let mut b = b.clone();
        if !spec.in_progress {
            b.blind = crate::engine::BlindRules { key: spec.blind_key.clone(), ..Default::default() };
            for l in &mut b.levels {
                l.played_this_round = 0;
            }
        }
        b
    }

    fn start_for(&self, spec: &Spec, b: &Board) -> RoundStart {
        let mut s = spec.start.clone();
        if !spec.in_progress {
            let f = b.rule_flags();
            spec.rules.apply(&mut s.deck, f.smeared, f.pareidolia);
        }
        s
    }

    /// Chance to win each round with `b`, plus total stats.
    fn odds(&self, b: &Board, added: &[&Joker], removed: &[&Joker], sims: usize) -> Vec<(f64, Stats)> {
        self.specs
            .iter()
            .map(|spec| {
                let bb = self.board_for(b, spec);
                let start = apply_mods(&self.start_for(spec, &bb), added, removed, !spec.in_progress);
                let n = if spec.horizon { (sims / 3).max(20) } else { sims };
                sim::round_odds(&bb, &start, n, self.opts.seed)
            })
            .collect()
    }

    fn odds_one(&self, b: &Board, spec: &Spec, sims: usize) -> (f64, Stats) {
        let bb = self.board_for(b, spec);
        sim::round_odds(&bb, &self.start_for(spec, &bb), sims, self.opts.seed)
    }

    fn typical(&self, b: &Board, added: &[&Joker], removed: &[&Joker]) -> Stats {
        let size = apply_mods(
            &RoundStart { hand: vec![], deck: vec![], hand_size: self.run.hand_size, hands: 1, discards: 0, scored: 0.0, target: 0.0 },
            added,
            removed,
            true,
        )
        .hand_size as usize;
        let mut bb = b.clone();
        bb.blind = Default::default();
        bb.hands_left = self.run.round_hands.max(1);
        bb.discards_left = self.run.round_discards;
        Stats::of(sim::typical_hands(&bb, &self.fresh_deck, size, self.opts.hand_samples, self.opts.seed))
    }
}

fn with_joker(b: &Board, j: Joker, at: usize) -> Board {
    let mut b = b.clone();
    if j.edition == Some(Edition::Negative) {
        b.joker_slots += 1;
    }
    b.jokers.insert(at.min(b.jokers.len()), j);
    b
}

fn without(b: &Board, i: usize) -> Board {
    let mut b = b.clone();
    let j = b.jokers.remove(i);
    if j.edition == Some(Edition::Negative) {
        b.joker_slots -= 1;
    }
    b
}

/// Runs `f` over `items` on all cores, keeping order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(items.len().max(1));
    let chunk = items.len().div_ceil(threads.max(1)).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = items.chunks(chunk).map(|c| s.spawn(|| c.iter().map(&f).collect::<Vec<R>>())).collect();
        handles.into_iter().flat_map(|h| h.join().expect("worker panicked")).collect()
    })
}

pub fn analyze(run: &RunState, data: &GameData, gold: Option<&GoldReport>, opts: &Options) -> Analysis {
    let t0 = Instant::now();
    let base = Board::from_run(run, data);
    let mut fresh_deck = run.full_deck();
    for c in &mut fresh_deck {
        c.debuff = false;
    }

    // Which rounds to simulate: the one in progress (or the next blind), and the boss.
    let mut specs = Vec::new();
    if let (true, Some(cb)) = (run.screen.in_blind(), &run.current_blind) {
        specs.push(Spec {
            label: "This blind".into(),
            blind_key: cb.key.clone(),
            blind_name: cb.name.clone(),
            start: RoundStart {
                hand: run.hand.clone(),
                deck: run.draw_pile.clone(),
                hand_size: run.hand_size,
                hands: run.hands_left,
                discards: run.discards_left,
                scored: cb.scored,
                target: cb.target,
            },
            rules: RoundRules::default(),
            in_progress: true,
            horizon: false,
        });
    }
    let blind_spec = |bl: &crate::save::BlindSlot| -> Spec {
        let mut start = RoundStart {
            hand: vec![],
            deck: fresh_deck.clone(),
            hand_size: run.hand_size,
            hands: run.round_hands,
            discards: run.round_discards,
            scored: 0.0,
            target: bl.target,
        };
        let rules = RoundRules::for_blind(&bl.key);
        start.hand_size += rules.hand_size_delta;
        match bl.key.as_str() {
            "bl_needle" => start.hands = 1,
            "bl_water" => start.discards = 0,
            _ => {}
        }
        Spec {
            label: if bl.slot == "Boss" { "Boss".into() } else { format!("Next: {}", bl.slot) },
            blind_key: bl.key.clone(),
            blind_name: bl.name.clone(),
            start,
            rules,
            in_progress: false,
            horizon: false,
        }
    };
    let upcoming: Vec<_> = run.blinds.iter().filter(|b| matches!(b.state.as_str(), "Select" | "Upcoming")).collect();
    for (i, bl) in upcoming.iter().enumerate() {
        if !(specs.is_empty() && i == 0) && bl.slot != "Boss" {
            continue;
        }
        specs.push(blind_spec(bl));
    }
    // Every upcoming blind, for the blind overview (base board only, so it's cheap)
    let overview_specs: Vec<(String, Spec)> = upcoming.iter().map(|bl| (bl.slot.clone(), blind_spec(bl))).collect();
    // Next ante: a plain boss (2x base, no effect), to see how far the board carries.
    for k in 1..=1 {
        let ante = run.ante + k;
        let target = crate::save::blind_amount(ante, run.blind_scaling) * 2.0 * run.ante_scaling;
        specs.push(Spec {
            label: format!("Ante {ante}"),
            blind_key: String::new(),
            blind_name: "plain boss".into(),
            start: RoundStart {
                hand: vec![],
                deck: fresh_deck.clone(),
                hand_size: run.hand_size,
                hands: run.round_hands,
                discards: run.round_discards,
                scored: 0.0,
                target,
            },
            rules: RoundRules::default(),
            in_progress: false,
            horizon: true,
        });
    }
    let ctx = Ctx { run, data, base, specs, fresh_deck, opts: opts.clone() };

    let base_odds = ctx.odds(&ctx.base, &[], &[], opts.sims);
    let base_typical = ctx.typical(&ctx.base, &[], &[]);
    let rounds: Vec<Round> = ctx
        .specs
        .iter()
        .zip(&base_odds)
        .map(|(s, (p, st))| Round {
            label: s.label.clone(),
            blind: s.blind_name.clone(),
            target: s.start.target,
            p_win: *p,
            total: *st,
            in_progress: s.in_progress,
            horizon: s.horizon,
            reach: st.mean / s.start.target.max(1.0),
            unmodelled: unmodelled_boss(&s.blind_key),
        })
        .collect();

    // Contributions
    let n = ctx.base.jokers.len();
    let idx: Vec<usize> = (0..n).collect();
    let contrib = par_map(&idx, |&i| {
        let b = without(&ctx.base, i);
        let removed = [&ctx.base.jokers[i]];
        let t = ctx.typical(&b, &[], &removed);
        let o = ctx.odds(&b, &[], &removed, opts.sims);
        (t, o)
    });
    let jokers: Vec<JokerReport> = (0..n)
        .map(|i| {
            let j = &ctx.base.jokers[i];
            let sj = &run.jokers[i];
            let (t, o) = &contrib[i];
            let share = if base_typical.mean > 0.0 { 1.0 - t.mean / base_typical.mean } else { 0.0 };
            JokerReport {
                key: j.key.clone(),
                name: data.name(&j.key).to_string(),
                rarity: rarity(data, &j.key),
                edition: j.edition,
                roles: roles(j),
                score_share: share,
                p_win_if_removed: o.iter().zip(&base_odds).map(|((p, _), (bp, _))| p - bp).collect(),
                eternal: sj.eternal,
                debuff: j.debuff,
                note: non_scoring_note(&j.key).map(str::to_string),
            }
        })
        .collect();

    let order = best_order(&ctx, base_typical.mean);
    // Which hands carry the points, from the hardest round (usually the boss)
    let hand_mix = ctx
        .specs
        .iter()
        .rfind(|s| !s.horizon)
        .map(|spec| {
            let bb = ctx.board_for(&ctx.base, spec);
            let start = ctx.start_for(spec, &bb);
            sim::round_hand_mix(&bb, &start, opts.sims.min(200), opts.seed)
                .into_iter()
                .map(|(h, share, played, mean)| HandShare { hand: h.name().to_string(), share, played, mean })
                .collect()
        })
        .unwrap_or_default();
    let missing = |k: &str| gold.is_some_and(|g| g.is_missing(k));

    // Shop jokers (and an open Buffoon pack)
    let mut offers: Vec<(Joker, i64)> = Vec::new();
    if let Some(shop) = &run.shop {
        for j in &shop.jokers {
            offers.push((Joker::from_save(j, data), j.cost));
        }
    }
    for c in run.open_pack.iter().filter(|c| c.set == "Joker") {
        if let Some(mut j) = Joker::from_key(&c.key, data) {
            j.edition = c.edition;
            offers.push((j, 0));
        }
    }
    let shop = par_map(&offers, |(j, cost)| evaluate_candidate(&ctx, j.clone(), *cost, &base_odds, base_typical.mean, opts.sims, true))
        .into_iter()
        .map(|mut c| {
            c.missing_gold = missing(&c.key);
            c
        })
        .collect();

    // Rescue: every joker the shop could still offer.
    let pool = shop_pool(run, data);
    let per_rarity: [usize; 4] = [0, 1, 2, 3].map(|r| pool.iter().filter(|k| data.center(k).and_then(|c| c.rarity) == Some(r as u8)).count());
    let joker_share = run.shop_rates.joker_share();
    let slots = run.shop_rates.slots.max(1) as i32;
    let candidates: Vec<Joker> = pool.iter().filter_map(|k| Joker::from_key(k, data)).collect();
    let screened = par_map(&candidates, |j| evaluate_candidate(&ctx, j.clone(), cost(data, &j.key), &base_odds, base_typical.mean, opts.screen_sims, false));
    let mut order_idx: Vec<usize> = (0..screened.len()).collect();
    // The hardest round of this ante (the last non-horizon one)
    let key_round = rounds.iter().rposition(|r| !r.horizon).unwrap_or(0);
    order_idx.sort_by(|&a, &b| rank_value(&screened[b], key_round).total_cmp(&rank_value(&screened[a], key_round)));
    let top: Vec<Joker> = order_idx.iter().take(opts.rescue_top).map(|&i| candidates[i].clone()).collect();
    let mut rescue: Vec<Candidate> = par_map(&top, |j| evaluate_candidate(&ctx, j.clone(), cost(data, &j.key), &base_odds, base_typical.mean, opts.sims, false));
    for c in &mut rescue {
        c.missing_gold = missing(&c.key);
        let r = data.center(&c.key).and_then(|x| x.rarity).unwrap_or(1) as usize;
        let rarity_p = [0.0, 0.7, 0.25, 0.05][r.min(3)];
        let per_card = joker_share * rarity_p / per_rarity[r.min(3)].max(1) as f64;
        c.per_shop = Some(1.0 - (1.0 - per_card).powi(slots));
    }
    rescue.sort_by(|a, b| rank_value(b, key_round).total_cmp(&rank_value(a, key_round)));
    let pool_entries: Vec<Candidate> = screened
        .iter()
        .map(|c| {
            let mut c = rescue.iter().find(|r| r.key == c.key).cloned().unwrap_or_else(|| c.clone());
            c.missing_gold = missing(&c.key);
            let r = data.center(&c.key).and_then(|x| x.rarity).unwrap_or(1) as usize;
            let per_card = joker_share * [0.0, 0.7, 0.25, 0.05][r.min(3)] / per_rarity[r.min(3)].max(1) as f64;
            c.per_shop = Some(1.0 - (1.0 - per_card).powi(slots));
            c
        })
        .collect();
    let blind_views: Vec<BlindView> = run
        .blinds
        .iter()
        .map(|bl| {
            let spec = overview_specs.iter().find(|(slot, _)| *slot == bl.slot).map(|(_, s)| s);
            BlindView {
                slot: bl.slot.clone(),
                name: bl.name.clone(),
                state: bl.state.clone(),
                target: bl.target,
                p_win: spec.map(|s| {
                    // reuse the main simulation where it's the same round
                    ctx.specs
                        .iter()
                        .position(|m| !m.horizon && !m.in_progress && m.blind_key == s.blind_key && m.start.target == s.start.target)
                        .map_or_else(|| ctx.odds_one(&ctx.base, s, opts.sims).0, |i| base_odds[i].0)
                }),
                reward: bl.reward,
                skip_tag: bl.skip_tag.as_ref().map(|k| TagView {
                    key: k.clone(),
                    name: data.tag(k).map_or_else(|| k.clone(), |t| t.name.clone()),
                }),
            }
        })
        .collect();

    let best_play = if run.screen.in_blind() && !run.hand.is_empty() {
        let mut b = ctx.base.clone();
        b.deck_remaining = run.draw_pile.len() as i64;
        sim::best_play(&b, &run.hand).map(|p| PlayAdvice {
            cards: p.cards.iter().map(|&i| run.hand[i].label()).collect(),
            hand: p.hand.name().to_string(),
            score: p.floor,
        })
    } else {
        None
    };

    Analysis {
        schema_version: 1,
        deck: run.deck.clone(),
        stake: run.stake,
        ante: run.ante,
        screen: format!("{:?}", run.screen),
        dollars: run.dollars,
        joker_slots: run.joker_slots,
        rounds,
        typical_hand: base_typical,
        hand_mix,
        jokers,
        order,
        shop,
        rescue,
        blinds: blind_views,
        pool: pool_entries,
        shop_odds: ShopOdds {
            joker_share,
            slots: run.shop_rates.slots,
            reroll_cost: run.shop.as_ref().map_or(5, |s| s.reroll_cost),
            pool_by_rarity: per_rarity,
            rarity_weight: [0.0, 0.7, 0.25, 0.05],
            money_per_hand: run.money_per_hand,
            interest_amount: run.interest_amount,
            interest_cap: run.interest_cap,
        },
        best_play,
        gold: gold.map(|g| GoldSummary { have: g.have_gold, total: g.total, this_run_counts: run.stake >= 8 }),
        caveats: run.snapshot.caveats.clone(),
        heuristics: vec![
            "Blind odds use a simple play/discard policy, not perfect play.",
            "Card order within a play: +Mult cards first, Glass/Polychrome last.",
            "Scaling jokers keep their current value; growth isn't projected.",
        ],
        save_age_secs: run.snapshot.age_secs,
        live: run.snapshot.live,
        elapsed_ms: t0.elapsed().as_millis(),
    }
}

/// What matters most: the chance to beat the hardest upcoming round, then the score gain.
/// Ranking: gain in win chance for this ante's hardest round, plus how much closer the
/// board gets to the next ante's boss, so a joker that only scrapes past this blind
/// doesn't top the list.
fn rank_value(c: &Candidate, now_round: usize) -> f64 {
    let now = c.p_win_delta.get(now_round).copied().unwrap_or(0.0);
    let later = c.reach_delta.iter().skip(now_round + 1).copied().fold(0.0, f64::max);
    now * 10.0 + later.min(1.0) * 5.0 + c.score_gain.min(5.0) * 0.1
}

fn rarity(data: &GameData, key: &str) -> String {
    RARITY_NAMES[data.center(key).and_then(|c| c.rarity).unwrap_or(0).min(4) as usize].to_string()
}

fn cost(data: &GameData, key: &str) -> i64 {
    data.center(key).map_or(0, |c| c.cost)
}

fn unmodelled_boss(key: &str) -> Option<String> {
    let s = match key {
        "bl_hook" => "The Hook's random discards",
        "bl_serpent" => "The Serpent's 3-card draws",
        "bl_pillar" => "The Pillar's debuffs",
        "bl_house" | "bl_wheel" | "bl_fish" | "bl_mark" => "face-down cards",
        "bl_tooth" => "The Tooth's money loss",
        "bl_final_leaf" => "Verdant Leaf (all cards debuffed until a joker is sold)",
        "bl_final_heart" => "Crimson Heart's disabled joker",
        "bl_final_bell" => "Cerulean Bell's forced card",
        "bl_final_acorn" => "Amber Acorn's joker shuffle",
        _ => return None,
    };
    Some(format!("not simulated: {s}"))
}

/// Jokers future shops can offer (`get_current_pool` culling).
///
/// `GAME.used_jokers` marks a joker while any copy of it exists (card.lua: set in
/// `set_ability`, cleared in `remove` once no copy is left). Shop and pack cards vanish
/// on reroll/leave, so only owned jokers stay excluded; Showman lifts even that.
pub fn shop_pool(run: &RunState, data: &GameData) -> Vec<String> {
    let showman = run.jokers.iter().any(|j| j.key == "j_ring_master");
    let deck = run.full_deck();
    data.jokers()
        .filter(|c| matches!(c.rarity, Some(1..=3)))
        .filter(|c| showman || !run.jokers.iter().any(|j| j.key == c.key))
        .filter(|c| c.no_pool_flag.as_ref().is_none_or(|f| !run.pool_flags.contains(f)))
        .filter(|c| c.yes_pool_flag.as_ref().is_none_or(|f| run.pool_flags.contains(f)))
        .filter(|c| !run.banned_keys.contains(&c.key))
        .filter(|c| {
            c.enhancement_gate.as_ref().is_none_or(|g| {
                let e = crate::model::Enhancement::from_key(g);
                deck.iter().any(|card| card.enhancement == e)
            })
        })
        .map(|c| c.key.clone())
        .collect()
}

/// Adds `j` (best of first/last slot) if there's room, otherwise the best single swap.
fn evaluate_candidate(ctx: &Ctx, j: Joker, cost: i64, base_odds: &[(f64, Stats)], base_mean: f64, sims: usize, full: bool) -> Candidate {
    let data = ctx.data;
    let base = &ctx.base;
    let room = (base.jokers.len() as i64) < base.joker_slots || j.edition == Some(Edition::Negative);
    let mut options: Vec<(String, Board, Vec<Joker>)> = Vec::new();
    if room {
        options.push(("add".into(), with_joker(base, j.clone(), base.jokers.len()), vec![]));
        if full && !base.jokers.is_empty() {
            options.push(("add (leftmost)".into(), with_joker(base, j.clone(), 0), vec![]));
        }
    } else {
        for (i, (cur, sj)) in base.jokers.iter().zip(&ctx.run.jokers).enumerate() {
            if sj.eternal {
                continue;
            }
            options.push((format!("replace {}", data.name(&cur.key)), with_joker(&without(base, i), j.clone(), i), vec![cur.clone()]));
        }
    }
    // Pick the option with the best typical score, then simulate the rounds for it.
    let mut best: Option<(String, Board, Vec<Joker>, Stats)> = None;
    for (label, b, removed) in options {
        let r: Vec<&Joker> = removed.iter().collect();
        let t = ctx.typical(&b, &[&j], &r);
        if best.as_ref().is_none_or(|(_, _, _, bt)| t.mean > bt.mean) {
            best = Some((label, b, removed, t));
        }
    }
    let Some((action, b, removed, t)) = best else {
        return Candidate {
            key: j.key.clone(),
            name: data.name(&j.key).to_string(),
            rarity: rarity(data, &j.key),
            cost,
            edition: j.edition,
            action: "no slot (all eternal)".into(),
            p_win: vec![],
            p_win_delta: vec![],
            reach: vec![],
            reach_delta: vec![],
            score_gain: 0.0,
            missing_gold: false,
            rarity_n: data.center(&j.key).and_then(|c| c.rarity).unwrap_or(0),
            precise: full,
            per_shop: None,
            roles: roles(&j),
            note: None,
        };
    };
    let r: Vec<&Joker> = removed.iter().collect();
    let odds = ctx.odds(&b, &[&j], &r, sims);
    Candidate {
        key: j.key.clone(),
        name: data.name(&j.key).to_string(),
        rarity: rarity(data, &j.key),
        cost,
        edition: j.edition,
        action,
        p_win_delta: odds.iter().zip(base_odds).map(|((p, _), (bp, _))| p - bp).collect(),
        p_win: odds.iter().map(|(p, _)| *p).collect(),
        reach: odds.iter().zip(&ctx.specs).map(|((_, st), sp)| st.mean / sp.start.target.max(1.0)).collect(),
        reach_delta: odds
            .iter()
            .zip(base_odds)
            .zip(&ctx.specs)
            .map(|(((_, st), (_, bst)), sp)| (st.mean - bst.mean) / sp.start.target.max(1.0))
            .collect(),
        score_gain: if base_mean > 0.0 { t.mean / base_mean - 1.0 } else { 0.0 },
        missing_gold: false,
        rarity_n: data.center(&j.key).and_then(|c| c.rarity).unwrap_or(0),
        precise: sims >= ctx.opts.sims,
        per_shop: None,
        roles: roles(&j),
        note: non_scoring_note(&j.key).map(str::to_string),
    }
}

/// Whether moving this joker around can change a score.
fn order_free(j: &Joker) -> bool {
    use Kind::*;
    let chips_only = matches!(
        j.kind,
        Banner | Stuntman | Castle | BlueJoker | Square | Runner | IceCream | StoneJoker | Bull | Wee | ScaryFace | Arrowhead | OddTodd
    ) || (j.kind == Other && j.t_chips > 0.0 && j.x_mult <= 1.0);
    let no_score = j.kind == Other && j.x_mult <= 1.0 && j.t_mult == 0.0 && j.t_chips == 0.0;
    let rule = matches!(j.kind, FourFingers | Shortcut | Smeared | Splash | Pareidolia | SockAndBuskin | HangingChad | Dusk | Seltzer | Hack | Mime);
    let edition_ok = matches!(j.edition, None | Some(Edition::Foil) | Some(Edition::Negative));
    (chips_only || no_score || rule) && edition_ok
}

/// Tries joker orders on fixed deals: each deal keeps the play that was best in the
/// current order, and only the scoring pass is repeated per order.
fn best_order(ctx: &Ctx, base_mean: f64) -> Option<OrderAdvice> {
    let jokers = &ctx.base.jokers;
    let n = jokers.len();
    if n < 2 {
        return None;
    }
    let copiers = jokers.iter().any(|j| matches!(j.kind, Kind::Blueprint | Kind::Brainstorm));
    let movable: Vec<usize> = (0..n).filter(|&i| copiers || !order_free(&jokers[i])).collect();
    if movable.len() < 2 {
        return Some(OrderAdvice { order: names(ctx, jokers), gain: 0.0, is_current: true });
    }
    // Fixed deals and plays
    let mut rng = crate::engine::Rng::new(ctx.opts.seed);
    let mut deck = ctx.fresh_deck.clone();
    let size = (ctx.run.hand_size.max(1) as usize).min(deck.len());
    let mut b0 = ctx.base.clone();
    b0.blind = Default::default();
    b0.hands_left = ctx.run.round_hands.max(1);
    b0.discards_left = ctx.run.round_discards;
    b0.deck_remaining = (deck.len() - size) as i64;
    let samples = ctx.opts.hand_samples.min(200);
    let mut plays: Vec<(Vec<Card>, Vec<Card>)> = Vec::with_capacity(samples);
    for _ in 0..samples {
        sim::shuffle(&mut deck, &mut rng);
        let hand = &deck[..size];
        if let Some(p) = sim::best_play(&b0, hand) {
            let played = p.cards.iter().map(|&i| hand[i]).collect();
            let held = (0..size).filter(|i| !p.cards.contains(i)).map(|i| hand[i]).collect();
            plays.push((played, held));
        }
    }
    let eval = |order: &[usize]| -> f64 {
        let mut b = b0.clone();
        b.jokers = order.iter().map(|&i| jokers[i].clone()).collect();
        plays.iter().map(|(p, h)| crate::engine::score(&b, p, h, &mut crate::engine::Unlucky, false).score).sum::<f64>()
            / plays.len().max(1) as f64
    };
    let current: Vec<usize> = (0..n).collect();
    let cur_val = eval(&current);
    // All permutations of the movable positions (at most 720)
    let mut perms = Vec::new();
    permute(&mut movable.clone(), 0, &mut perms);
    let orders: Vec<Vec<usize>> = perms
        .into_iter()
        .map(|p| {
            let mut o = current.clone();
            for (slot, &j) in movable.iter().zip(&p) {
                o[*slot] = j;
            }
            o
        })
        .collect();
    let vals = par_map(&orders, |o| eval(o));
    let (bi, bv) = vals.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(i, v)| (i, *v))?;
    let _ = base_mean;
    let gain = if cur_val > 0.0 { bv / cur_val - 1.0 } else { 0.0 };
    if gain < 0.005 {
        return Some(OrderAdvice { order: names(ctx, jokers), gain: 0.0, is_current: true });
    }
    let best: Vec<Joker> = orders[bi].iter().map(|&i| jokers[i].clone()).collect();
    Some(OrderAdvice { order: names(ctx, &best), gain, is_current: false })
}

fn names(ctx: &Ctx, js: &[Joker]) -> Vec<String> {
    js.iter().map(|j| ctx.data.name(&j.key).to_string()).collect()
}

fn permute(v: &mut Vec<usize>, k: usize, out: &mut Vec<Vec<usize>>) {
    if k == v.len() {
        out.push(v.clone());
        return;
    }
    for i in k..v.len() {
        v.swap(k, i);
        permute(v, k + 1, out);
        v.swap(k, i);
    }
}
