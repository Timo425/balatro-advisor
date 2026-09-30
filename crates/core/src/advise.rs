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
    /// Rounds left if perishable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perishable: Option<i64>,
    pub rental: bool,
    pub debuff: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The game's own text for it, from the local install, with its current values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    /// For jokers that grow or fade: their value one ante from now, under a stated assumption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub growth: Option<Growth>,
    /// How much stronger it makes your board by Ante 8 (best-hand score ratio, both boards
    /// projected: growers grown, faders faded, perishables gone). An estimate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_mult: Option<f64>,
    /// Set when the joker to sell was picked for the long run rather than for this ante.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Growth {
    /// e.g. "+6 Mult after 1 ante (2 skipped packs)"
    pub assumption: String,
    /// Next-ante reach at the grown size (compare with `reach` at today's size).
    pub reach: f64,
    pub fades: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayAdvice {
    pub cards: Vec<String>,
    pub hand: String,
    pub score: f64,
    /// A tip about how to play the round (e.g. burn discards for Mystic Summit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
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
    /// Planets, packs and rerolls, valued against `options_round`.
    pub options: Vec<ShopOption>,
    pub options_round: usize,
    /// Every tarot, valued for this run (the dig list's Tarots tab).
    pub tarots: Vec<TarotValue>,
    /// What the "by Ante 8" numbers assume.
    pub long_assumptions: String,
    pub outlook: Option<Outlook>,
    /// Style groups (name → joker keys), for tagging jokers in the page.
    pub style_groups: Vec<(String, Vec<String>)>,
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

/// "Which play style has the most room?": each style's best 2-joker addition from the
/// current pool, measured against a boss two antes ahead. An estimate: it can't know what
/// you'll actually find, but every style is compared on the same assumptions.
#[derive(Debug, Clone, Serialize)]
pub struct Outlook {
    pub target_label: String,
    pub target: f64,
    /// Share of that target your current board reaches (mean round total / target).
    pub now_reach: f64,
    pub styles: Vec<Style>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Style {
    pub name: String,
    /// The style your points currently come from.
    pub current: bool,
    /// Share of your points that come from this style's hands now.
    pub points_now: f64,
    /// Jokers you own that belong to it.
    pub owned: Vec<String>,
    /// The best 2 it could add from the pool (name, chance per shop, cost).
    pub add: Vec<(String, f64, i64)>,
    /// Jokers those would replace when slots are full.
    pub replaces: Vec<String>,
    pub reach_one: f64,
    pub reach: f64,
}

/// Something in (or from) the shop, valued as the win chance for one round afterwards.
#[derive(Debug, Clone, Serialize)]
pub struct ShopOption {
    pub label: String,
    /// planet | pack | reroll | voucher
    pub kind: String,
    pub cost: i64,
    /// Win chance for `round` after taking this option (expected value for packs and rerolls).
    /// Equal to the current chance for options that don't change this round (economy vouchers).
    pub p_win: f64,
    pub note: String,
    /// Money left after paying, and the interest that money earns per round (before payouts).
    pub money_after: f64,
    pub interest_now: i64,
    pub interest_after: i64,
    /// You don't have the money for it right now.
    pub unaffordable: bool,
    /// Money it gives back (money tarots), counted in `money_after`.
    #[serde(default)]
    pub money_gain: f64,
    /// Share of this round's target a typical round scores after taking it, where known:
    /// breaks ties when win chances saturate (Mr. Bones, easy blinds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reach: Option<f64>,
    /// Board strength by Ante 8 relative to not taking it (see `Candidate::long_mult`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_mult: Option<f64>,
    /// For jokers: the key (style tags in the page) and the game text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
}

/// Interest the game pays on `money` (`$1` per `$5`, capped; state_events.lua end of round).
pub fn interest(money: f64, amount: i64, cap: i64) -> i64 {
    if money < 5.0 {
        return 0;
    }
    amount * ((money / 5.0).floor() as i64).min(cap / 5)
}

/// A tarot's value for this run: used the way a player sensibly would, on your real deck.
#[derive(Debug, Clone, Serialize)]
pub struct TarotValue {
    pub key: String,
    pub name: String,
    /// Win chance for the options round after using it (= now when not simulated).
    pub p_win: f64,
    /// What it was assumed to do, or why it isn't simulated.
    pub note: String,
    pub simulated: bool,
    /// Chance a given shop shows it in a card slot.
    pub per_shop: f64,
    /// Share of the next ante's boss target reached after using it (card changes are
    /// subtle, so this is the ranking; win chances saturate).
    pub reach: f64,
    pub reach_now: f64,
    /// Money it gives (Hermit, Temperance), for "money after" in the options list.
    pub money_gain: f64,
    /// The deck after using it, for the By Ante 8 projection (deck-changing tarots).
    #[serde(skip)]
    pub deck: Option<Vec<Card>>,
}

/// One of this ante's three blinds.
#[derive(Debug, Clone, Serialize)]
pub struct BlindView {
    pub slot: String,
    pub name: String,
    /// What the boss does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
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
        "j_mr_bones" => "Survival: saves a lost round once if you scored at least 25% of the blind, then destroys itself (counted in win chances, not in scores)",
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

/// Fresh deals per board in the quick screen of the whole pool.
const SCREEN_DEALS: usize = 100;

struct Ctx<'a> {
    run: &'a RunState,
    /// Current jokers' share of the best-hand score (filled after the contribution pass).
    shares: Vec<f64>,
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
        self.typical_n(b, added, removed, self.opts.hand_samples)
    }

    /// Deals used by the quick screen (the first `SCREEN_DEALS` of the same sequence).
    fn typical_n(&self, b: &Board, added: &[&Joker], removed: &[&Joker], samples: usize) -> Stats {
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
        // With Mystic Summit (and no Banner) you'd burn your discards first, so hands are
        // played with none left
        let mystic = b.jokers.iter().any(|j| j.kind == Kind::MysticSummit) && !b.jokers.iter().any(|j| j.kind == Kind::Banner);
        bb.discards_left = if mystic { 0 } else { self.run.round_discards };
        Stats::of(sim::typical_hands(&bb, &self.fresh_deck, size, samples, self.opts.seed))
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
    let timing = std::env::var_os("BAV_TIMING").is_some();
    let lap = |what: &str| {
        if timing {
            eprintln!("{:>6} ms  {what}", t0.elapsed().as_millis());
        }
    };
    let base = Board::from_run(run, data);
    let dctx = desc_ctx(run);
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
    let mut ctx = Ctx { run, data, base, specs, fresh_deck, opts: opts.clone(), shares: Vec::new() };

    lap("setup");
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

    lap("base odds + typical");
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
                perishable: sj.perishable,
                rental: sj.rental,
                debuff: j.debuff,
                note: non_scoring_note(&j.key).map(str::to_string),
                desc: describe(&j.key, &sj.ability, &dctx),
            }
        })
        .collect();

    lap("contributions");
    ctx.shares = jokers.iter().map(|j| j.score_share).collect();
    let ctx = ctx;
    let order = best_order(&ctx, base_typical.mean);
    // Which hands carry the points, from the hardest round (usually the boss)
    let hand_mix: Vec<HandShare> = ctx
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

    lap("order + hand mix");
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
    let shop = par_map(&offers, |(j, cost)| evaluate_candidate(&ctx, j.clone(), *cost, &base_odds, base_typical.mean, opts.sims, true, None))
        .into_iter()
        .map(|mut c| {
            c.missing_gold = missing(&c.key);
            c
        })
        .collect();

    lap("shop jokers");
    // Rescue: every joker the shop could still offer.
    let pool = shop_pool(run, data);
    let per_rarity: [usize; 4] = [0, 1, 2, 3].map(|r| pool.iter().filter(|k| data.center(k).and_then(|c| c.rarity) == Some(r as u8)).count());
    let joker_share = run.shop_rates.joker_share();
    let slots = run.shop_rates.slots.max(1) as i32;
    let candidates: Vec<Joker> = pool.iter().filter_map(|k| Joker::from_key(k, data)).collect();
    let screen_base = ctx.typical_n(&ctx.base, &[], &[], SCREEN_DEALS).mean;
    let screened = par_map(&candidates, |j| evaluate_candidate(&ctx, j.clone(), cost(data, &j.key), &base_odds, screen_base, opts.screen_sims, false, None));
    lap("screen pool");
    let mut order_idx: Vec<usize> = (0..screened.len()).collect();
    // The hardest round of this ante (the last non-horizon one)
    let key_round = rounds.iter().rposition(|r| !r.horizon).unwrap_or(0);
    order_idx.sort_by(|&a, &b| rank_value(&screened[b], key_round).total_cmp(&rank_value(&screened[a], key_round)));
    let top: Vec<Joker> = order_idx.iter().take(opts.rescue_top).map(|&i| candidates[i].clone()).collect();
    let mut rescue: Vec<Candidate> = par_map(&top, |j| evaluate_candidate(&ctx, j.clone(), cost(data, &j.key), &base_odds, base_typical.mean, opts.sims, false, None));
    for c in &mut rescue {
        c.missing_gold = missing(&c.key);
        let r = data.center(&c.key).and_then(|x| x.rarity).unwrap_or(1) as usize;
        let rarity_p = [0.0, 0.7, 0.25, 0.05][r.min(3)];
        let per_card = joker_share * rarity_p / per_rarity[r.min(3)].max(1) as f64;
        c.per_shop = Some(1.0 - (1.0 - per_card).powi(slots));
    }
    rescue.sort_by(|a, b| rank_value(b, key_round).total_cmp(&rank_value(a, key_round)));
    lap("refine top");
    let mut pool_entries: Vec<Candidate> = screened
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
    // Growing / fading jokers: re-simulated at their size one ante from now
    let growth_models: Vec<(usize, Joker, String, bool)> = pool_entries
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let j = Joker::from_key(&c.key, data)?;
            // Money left after buying it is what could pay for its growth
            let (grown, label, fades) = grow_one_ante(&j, &hand_mix, run.dollars - c.cost as f64, run.interest_cap as f64)?;
            Some((i, grown, label, fades))
        })
        .collect();
    let hz_idx = ctx.specs.iter().position(|x| x.horizon);
    let grown: Vec<Option<f64>> = par_map(&growth_models, |(_, j, _, _)| {
        let h = hz_idx?;
        let c = evaluate_candidate(&ctx, j.clone(), 0, &base_odds, screen_base, opts.screen_sims, false, None);
        c.reach.get(h).copied()
    });
    for ((i, _, label, fades), r) in growth_models.into_iter().zip(grown) {
        if let Some(r) = r {
            pool_entries[i].growth = Some(Growth { assumption: label, reach: r, fades });
        }
    }
    let blind_views: Vec<BlindView> = run
        .blinds
        .iter()
        .map(|bl| {
            let spec = overview_specs.iter().find(|(slot, _)| *slot == bl.slot).map(|(_, s)| s);
            BlindView {
                slot: bl.slot.clone(),
                name: bl.name.clone(),
                effect: boss_effect(&bl.key).map(str::to_string),
                state: bl.state.clone(),
                target: bl.target,
                p_win: if bl.state == "Current" {
                    // In progress: the simulation that starts from the real hand, draw pile and score so far
                    ctx.specs.iter().position(|m| m.in_progress).map(|i| base_odds[i].0)
                } else {
                    spec.map(|s| {
                        // reuse the main simulation where it's the same round
                        ctx.specs
                            .iter()
                            .position(|m| !m.horizon && !m.in_progress && m.blind_key == s.blind_key && m.start.target == s.start.target)
                            .map_or_else(|| ctx.odds_one(&ctx.base, s, opts.sims).0, |i| base_odds[i].0)
                    })
                },
                reward: bl.reward,
                skip_tag: bl.skip_tag.as_ref().map(|k| TagView {
                    key: k.clone(),
                    name: data.tag(k).map_or_else(|| k.clone(), |t| t.name.clone()),
                }),
            }
        })
        .collect();

    lap("blind views");
    // Game text for every joker shown: the shop's own copies, else a fresh one from its config
    let shop_ability = |key: &str| run.shop.as_ref().and_then(|s| s.jokers.iter().find(|j| j.key == key)).map(|j| j.ability.clone());
    let fill = |list: &mut Vec<Candidate>| {
        for c in list.iter_mut() {
            let ab = shop_ability(&c.key)
                .or_else(|| data.center(&c.key).map(|x| crate::engine::joker::ability_from_config(&x.config)))
                .unwrap_or_default();
            c.desc = describe(&c.key, &ab, &dctx);
        }
    };
    let mut pool_entries = pool_entries;
    let mut shop = shop;
    let mut rescue = rescue;
    fill(&mut pool_entries);
    fill(&mut shop);
    fill(&mut rescue);
    // By Ante 8: every board projected to then (growers grown, faders faded, perishables
    // that run out gone, planets bought with the money it leaves you), compared on whole
    // simulated rounds. Money is the one channel economy flows through, in two forms:
    // money you hold every ante (rent lowers it) sets how many pack skips, rerolls and
    // planets you buy each ante; one-off money (a price, a sell-back, Temperance) buys
    // them once. Nothing is valued twice.
    let antes_left = ((run.win_ante - run.ante) as f64 + 0.5).max(0.5);
    let line = run.interest_cap as f64;
    // Rent ($3 a round) comes out of the money that would buy planets, rerolls and pack
    // skips: each rental counts as one ante of rent less money held.
    const RENT_PER_ANTE: f64 = 9.0;
    let top_hand = hand_mix.first().and_then(|h| crate::engine::HandType::from_name(&h.hand));
    let lasts = |sj: &crate::save::JokerCard| sj.perishable.is_none_or(|r| r as f64 >= 3.0 * antes_left);
    let owned_rent = |keep: &dyn Fn(usize) -> bool| {
        run.jokers.iter().enumerate().filter(|(i, sj)| keep(*i) && lasts(sj) && sj.rental).count() as f64 * RENT_PER_ANTE
    };
    // Planets on the hand that earns most of your points: half a level per ante as a base,
    // plus more when money sits above the interest line (runs vary from 0 to 10+ levels).
    let levels_for = |dollars: f64| {
        let per_ante = (0.5 + (dollars - line).max(0.0) / 20.0).min(2.0);
        (per_ante, (per_ante * antes_left).round() as i64)
    };
    // The projected board: the jokers kept (by index), grown with the money you'd hold.
    let project = |keep: &dyn Fn(usize) -> bool, dollars: f64| -> Board {
        let dollars = dollars - owned_rent(keep);
        let mut b = ctx.base.clone();
        b.blind = Default::default();
        if let Some(top) = top_hand {
            let l = b.levels[top as usize];
            b.levels[top as usize] = l.with_level(l.level + levels_for(dollars).1);
        }
        b.jokers = ctx
            .base
            .jokers
            .iter()
            .zip(&run.jokers)
            .enumerate()
            .filter(|(i, (_, sj))| keep(*i) && lasts(sj))
            .map(|(_, (j, _))| grow_antes(j, &hand_mix, dollars, line, antes_left).map_or_else(|| j.clone(), |g| g.0))
            .collect();
        b
    };
    // Stand-ins for the jokers you'd find over the run: empty slots get alternating ×1.5,
    // +60 Chips and +15 Mult jokers, and the option is compared against a ×1.25 "typical
    // find" in its slot. An assumption, labelled as one.
    let stand_in = |x: f64, m: f64, chips: f64| {
        let mut j = Joker::from_key("j_joker", data).expect("j_joker in data");
        j.mult = 0.0;
        if x > 1.0 {
            j.kind = Kind::Other;
            j.x_mult = x;
        } else if chips > 0.0 {
            j.kind = Kind::Stuntman;
            j.extra.chip_mod = chips;
        } else {
            j.mult = m;
        }
        j.key = "stand-in".into();
        j
    };
    // One-off money: spent once on pack skips / rerolls for a joker that grows from them
    // (about $5 each), else on planets for your main hand (about $5 a level).
    let spend_once = |b: &mut Board, once: f64| {
        let buys = once / 5.0;
        if let Some(j) = b.jokers.iter_mut().find(|j| j.key == "j_red_card" || j.key == "j_flash") {
            let per = if j.key == "j_red_card" { 3.0 } else { 2.0 };
            j.mult = (j.mult + per * buys).max(0.0);
        } else if let Some(top) = top_hand {
            let l = b.levels[top as usize];
            b.levels[top as usize] = l.with_level((l.level + buys.round() as i64).max(1));
        }
    };
    let fill_long = |mut b: Board, option: Option<Joker>, once: f64| -> Board {
        b.jokers.push(option.unwrap_or_else(|| stand_in(1.25, 0.0, 0.0)));
        spend_once(&mut b, once);
        let mut k = 0;
        while (b.jokers.len() as i64) < b.joker_slots {
            b.jokers.push(match k % 3 {
                0 => stand_in(1.5, 0.0, 0.0),
                1 => stand_in(1.0, 0.0, 60.0),
                _ => stand_in(1.0, 15.0, 0.0),
            });
            k += 1;
        }
        b
    };
    // Whole rounds with no target (all hands played, discards and flush chases included),
    // so builds that dig for their hand count the way they're played.
    let long_spec = Spec {
        label: "Ante 8 projection".into(),
        blind_key: String::new(),
        blind_name: String::new(),
        start: RoundStart {
            hand: vec![],
            deck: ctx.fresh_deck.clone(),
            hand_size: run.hand_size,
            hands: run.round_hands,
            discards: run.round_discards,
            scored: 0.0,
            target: 1e300,
        },
        rules: RoundRules::default(),
        in_progress: false,
        horizon: true,
    };
    let long_score = |b: &Board| ctx.odds_one(b, &long_spec, 48).1.mean.max(1.0);
    let l0 = long_score(&fill_long(project(&|_| true, run.dollars), None, 0.0));
    let lasting = run.jokers.iter().filter(|sj| lasts(sj)).count() as i64;
    let full = lasting >= ctx.base.joker_slots;
    // Baselines with a typical find in place of each joker you could sell
    let sell_base: Vec<Option<f64>> = if full {
        par_map(&(0..run.jokers.len()).collect::<Vec<_>>(), |&i| {
            (!run.jokers[i].eternal && lasts(&run.jokers[i])).then(|| long_score(&fill_long(project(&|k| k != i, run.dollars), None, 0.0)))
        })
    } else {
        vec![]
    };
    // An option's projected score: bought for `cost` (selling `sell`, if any), rent paid if rental.
    let long_of = |j: &Joker, sell: Option<usize>, cost: i64, rental: bool| -> f64 {
        let back = sell.map_or(0, |i| run.jokers[i].sell_value) as f64;
        let dollars = run.dollars - if rental { RENT_PER_ANTE } else { 0.0 };
        let horizon = if j.key == "j_madness" { 1.0 } else { antes_left };
        let g = grow_antes(j, &hand_mix, dollars, line, horizon).map_or_else(|| j.clone(), |g| g.0);
        long_score(&fill_long(project(&|k| Some(k) != sell, dollars), Some(g), back - cost as f64))
    };
    let sell_index = |action: &str| {
        action.strip_prefix("replace ").map(|n| n.trim_end_matches(", put it rightmost")).and_then(|name| ctx.base.jokers.iter().position(|x| data.name(&x.key) == name))
    };
    let ratio = |v: f64, sell: Option<usize>| v / sell.and_then(|i| sell_base.get(i).copied().flatten()).unwrap_or(l0);
    let long_mults: Vec<Option<f64>> = par_map(&pool_entries, |c| {
        let mut j = Joker::from_key(&c.key, data)?;
        j.edition = c.edition;
        // Room by Ante 8 (perishables gone) means nothing needs selling then
        let sell = if full { sell_index(&c.action) } else { None };
        if full && sell.is_none() && j.edition != Some(Edition::Negative) {
            return None;
        }
        Some(ratio(long_of(&j, sell, c.cost, false), sell))
    });
    for (c, m) in pool_entries.iter_mut().zip(long_mults) {
        c.long_mult = m;
    }
    // Jokers on offer: when slots are full, the one to sell is picked for the long run
    // (growers and lasting jokers kept), unless that costs more than 5 points of win chance
    // in this ante's hardest round.
    const NOW_SLACK: f64 = 0.05;
    for (c, (j, _)) in shop.iter_mut().zip(&offers) {
        let sticker = run.shop.as_ref().and_then(|sh| sh.jokers.iter().find(|x| x.key == c.key));
        if sticker.and_then(|x| x.perishable).is_some_and(|r| (r as f64) < 3.0 * antes_left) {
            c.long_mult = Some(1.0); // gone before then
            continue;
        }
        let rental = sticker.is_some_and(|x| x.rental);
        let today = sell_index(&c.action);
        if !full || j.edition == Some(Edition::Negative) {
            c.long_mult = Some(ratio(long_of(j, None, c.cost, rental), None));
            continue;
        }
        let Some(today) = today else { continue };
        let tries: Vec<(usize, f64)> = par_map(&(0..run.jokers.len()).filter(|&i| sell_base.get(i).is_some_and(|b| b.is_some())).collect::<Vec<_>>(), |&i| {
            (i, long_of(j, Some(i), c.cost, rental))
        });
        let Some(&(best, best_v)) = tries.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else { continue };
        let today_v = tries.iter().find(|t| t.0 == today).map_or(best_v, |t| t.1);
        c.long_mult = Some(ratio(today_v, Some(today)));
        if best == today {
            continue;
        }
        let alt = evaluate_candidate(&ctx, j.clone(), c.cost, &base_odds, base_typical.mean, opts.sims, true, Some(best));
        let now_p = |x: &Candidate| x.p_win.get(key_round).copied().unwrap_or(0.0);
        let (keep_name, sell_name) = (data.name(&ctx.base.jokers[today].key), data.name(&ctx.base.jokers[best].key));
        if now_p(&alt) >= now_p(c) - NOW_SLACK {
            let note = format!("sell {sell_name} for it, not {keep_name}: better by Ante 8 (selling {keep_name} scores a bit more this ante)");
            *c = Candidate { missing_gold: c.missing_gold, desc: c.desc.take(), long_mult: Some(ratio(best_v, Some(best))), sell_note: Some(note), ..alt };
        } else {
            c.sell_note = Some(format!(
                "by Ante 8, selling {sell_name} instead would be better (×{:.2}), but costs {:.0} points of win chance now",
                ratio(best_v, Some(best)),
                (now_p(c) - now_p(&alt)) * 100.0
            ));
        }
    }
    lap("by ante 8");
    let (mut options, tarots) = shop_options(&ctx, run, data, &pool_entries, &base_odds, key_round, per_rarity, joker_share, &shop);
    lap("shop options");
    // Planets and money cards, with the money they cost or give
    let long_idx: Vec<usize> = options.iter().enumerate().filter(|(_, o)| o.kind == "planet" || o.money_gain > 0.0).map(|(i, _)| i).collect();
    let long_opts: Vec<Option<f64>> = par_map(&long_idx, |&i| {
        let o = &options[i];
        let mut b = fill_long(project(&|_| true, run.dollars), None, o.money_after - run.dollars);
        if o.kind == "planet" {
            let hand = o.key.as_ref().and_then(|k| data.center(k)).and_then(|c| c.config.get("hand_type")).and_then(|v| v.as_str()).and_then(crate::engine::HandType::from_name);
            let h = hand?;
            let l = b.levels[h as usize];
            b.levels[h as usize] = l.with_level(l.level + 1);
        }
        Some(long_score(&b) / l0)
    });
    for (i, m) in long_idx.into_iter().zip(long_opts) {
        options[i].long_mult = m;
    }
    // Random jokers (Judgement, rerolls, Buffoon packs): the best one seen, and one you
    // don't want is sold, so a draw is worth at least a typical find (×1.00). Its price
    // comes off as one-off money.
    let mut rng = crate::engine::Rng::new(opts.seed ^ 0x10ae);
    for o in options.iter_mut() {
        let (cards, share) = match (o.kind.as_str(), o.key.as_deref()) {
            ("tarot", Some("c_judgement")) => (1, 1.0),
            ("reroll", _) => (run.shop_rates.slots.max(1) as usize, joker_share),
            ("pack", _) if o.label.contains("Buffoon") => (if o.label.contains("Jumbo") || o.label.contains("Mega") { 4 } else { 2 }, 1.0),
            _ => continue,
        };
        let draw = long_draw(&pool_entries, cards, share, &mut rng);
        let price = long_score(&fill_long(project(&|_| true, run.dollars), None, -(o.cost as f64))) / l0;
        o.long_mult = Some(draw * price);
    }
    // Tarots: a changed deck is permanent, so it's projected like everything else; money
    // tarots through money; Judgement as a random joker. Arcana packs take the best card
    // in them, or the skip when Red Card grows from it (+3 Mult).
    // Deck changes are small, so they get more rounds than the rest (and the same seeds).
    const TAROT_ROUNDS: usize = 120;
    // A board without a chips joker will likely find one by Ante 8: the typical find in
    // these projections is then a +60 Chips joker, so card chips (Bonus, Stone) aren't
    // valued as if that gap stayed open.
    const CHIP_JOKERS: &[&str] = &["j_stuntman", "j_bull", "j_banner", "j_scary_face", "j_arrowhead", "j_castle", "j_runner", "j_square",
        "j_wee", "j_ice_cream", "j_blue_joker", "j_sly", "j_wily", "j_clever", "j_devious", "j_crafty", "j_odd_todd", "j_stone", "j_hiker"];
    let has_chips = ctx.base.jokers.iter().any(|j| CHIP_JOKERS.contains(&j.key.as_str()));
    let deck_long = |d: &[Card], dollars: f64| -> Stats {
        let find = if has_chips { None } else { Some(stand_in(1.0, 0.0, 60.0)) };
        let mut b = fill_long(project(&|_| true, dollars), find, 0.0);
        let tally = |e: crate::model::Enhancement| d.iter().filter(|c| c.enhancement == Some(e)).count() as i64;
        b.steel_tally = tally(crate::model::Enhancement::Steel);
        b.stone_tally = tally(crate::model::Enhancement::Stone);
        b.driver_tally = d.iter().filter(|c| c.enhancement.is_some()).count() as i64;
        b.playing_cards = d.len() as i64;
        let mut sp = long_spec.clone();
        sp.start.deck = d.to_vec();
        ctx.odds_one(&b, &sp, TAROT_ROUNDS).1
    };
    let deck_base = deck_long(&ctx.fresh_deck, run.dollars);
    let tarot_long: Vec<f64> = par_map(&tarots, |t| {
        if let Some(d) = &t.deck {
            let mut st = deck_long(d, run.dollars);
            // Money the new cards earn while scoring (Lucky cards' $20, gold seals) is money
            // you get every round: about 3 rounds an ante.
            let extra = (st.money - deck_base.money) * 3.0;
            if extra.abs() > 0.5 {
                st = deck_long(d, run.dollars + extra);
            }
            st.mean.max(1.0) / deck_base.mean.max(1.0)
        } else if t.money_gain > 0.0 {
            long_score(&fill_long(project(&|_| true, run.dollars), None, t.money_gain)) / l0
        } else if t.key == "c_judgement" {
            long_draw(&pool_entries, 1, 1.0, &mut crate::engine::Rng::new(opts.seed ^ 0x1d6e))
        } else {
            1.0
        }
    });
    let skip_long = {
        let mut b = fill_long(project(&|_| true, run.dollars), None, 0.0);
        match b.jokers.iter_mut().find(|j| j.key == "j_red_card") {
            Some(j) => {
                j.mult += 3.0;
                long_score(&b) / l0
            }
            None => 1.0,
        }
    };
    let price = |cost: i64| if cost == 0 { 1.0 } else { long_score(&fill_long(project(&|_| true, run.dollars), None, -(cost as f64))) / l0 };
    for o in options.iter_mut() {
        if o.kind == "tarot" && o.money_gain == 0.0 {
            if let Some(i) = o.key.as_ref().and_then(|k| tarots.iter().position(|t| &t.key == k)) {
                o.long_mult = Some(tarot_long[i] * price(o.cost));
            }
        } else if o.kind == "pack" && o.label.contains("Arcana") {
            let k = if o.label.contains("Jumbo") || o.label.contains("Mega") { 5 } else { 3 };
            let vals: Vec<f64> = tarot_long.iter().map(|v| v.max(skip_long)).collect();
            o.long_mult = Some(best_of_subsets(&vals, k).0 * price(o.cost));
            if skip_long > 1.0 {
                o.note = format!("{} · or skip it for Red Card +3 Mult (×{skip_long:.2} by Ante 8)", o.note);
            }
        }
    }
    let base_reach = base_odds.get(key_round).zip(ctx.specs.get(key_round)).map_or(0.0, |(o, sp)| o.1.mean / sp.start.target.max(1.0));
    rank_options(&mut options, base_reach);
    let (levels_per_ante, planet_levels) = levels_for(run.dollars - owned_rent(&|_| true));
    let long_note = format!(
        "Your board projected {antes_left:.1} antes ahead: growing jokers grown, fading ones faded, perishables that run out dropped, {} +{planet_levels} levels (about {levels_per_ante:.1} per ante with your money), empty slots filled with stand-in jokers (×1.5, +60 Chips, +15 Mult). Each option is compared with a typical find (×1.25) in its slot, on whole simulated rounds, with the money it leaves you: its price, what selling a joker gives back or a money card gives (spent once, on pack skips for Red Card/Flash or on planets, ~$5 each), and $9 less money held every ante per rental. ×1.00 = as good as a typical find.",
        top_hand.map_or("your main hand", |h| h.name())
    );
    let shares: Vec<f64> = jokers.iter().map(|j| j.score_share).collect();
    let outlook = archetype_outlook(&ctx, run, data, &pool_entries, &shares, &hand_mix);

    lap("outlook");
    let best_play = if run.screen.in_blind() && !run.hand.is_empty() {
        let mut b = ctx.base.clone();
        b.deck_remaining = run.draw_pile.len() as i64;
        let has = |k: Kind| b.jokers.iter().any(|j| j.kind == k && !j.debuff);
        let tip = (run.discards_left > 0 && has(Kind::MysticSummit) && !has(Kind::Banner)).then(|| {
            format!(
                "Use your {} discard{} first on cards outside this play: Mystic Summit gives +15 Mult on every hand once none are left",
                run.discards_left,
                if run.discards_left > 1 { "s" } else { "" }
            )
        });
        sim::best_play(&b, &run.hand).map(|p| PlayAdvice {
            cards: p.cards.iter().map(|&i| run.hand[i].label()).collect(),
            hand: p.hand.name().to_string(),
            score: p.floor,
            tip,
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
        options,
        options_round: key_round,
        tarots,
        long_assumptions: long_note.clone(),
        outlook,
        style_groups: archetypes().into_iter().map(|(n, m, _)| (n.to_string(), m.iter().map(|k| k.to_string()).collect())).collect(),
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
/// With `sell`, only that swap is tried.
#[allow(clippy::too_many_arguments)]
fn evaluate_candidate(ctx: &Ctx, j: Joker, cost: i64, base_odds: &[(f64, Stats)], base_mean: f64, sims: usize, full: bool, sell: Option<usize>) -> Candidate {
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
        // A perished joker (debuffed, 0 rounds left) is dead weight: it's the one to sell.
        let perished = ctx.run.jokers.iter().position(|sj| sj.perishable == Some(0) && !sj.eternal);
        // Screening only tries the weakest non-eternal joker; the full pass tries every swap.
        // Ties in score go against jokers that don't score but do something else (Mr. Bones).
        let keeps_value = |i: usize| base.jokers.get(i).is_some_and(|j| j.key == "j_mr_bones") as u8;
        let weakest = perished.or_else(|| {
            ctx.shares.iter().enumerate()
                .filter(|(i, _)| ctx.run.jokers.get(*i).is_some_and(|sj| !sj.eternal))
                .min_by(|a, b| a.1.total_cmp(b.1).then(keeps_value(a.0).cmp(&keeps_value(b.0))))
                .map(|(i, _)| i)
        });
        // Mr. Bones scores nothing but saves a run: he's only sold when nothing else can be.
        let others = ctx.run.jokers.iter().zip(&base.jokers).filter(|(sj, j)| !sj.eternal && j.key != "j_mr_bones").count();
        let weakest = weakest.filter(|&i| others == 0 || keeps_value(i) == 0).or_else(|| {
            ctx.shares.iter().enumerate()
                .filter(|(i, _)| ctx.run.jokers.get(*i).is_some_and(|sj| !sj.eternal) && keeps_value(*i) == 0)
                .min_by(|a, b| a.1.total_cmp(b.1))
                .map(|(i, _)| i)
        });
        for (i, (cur, sj)) in base.jokers.iter().zip(&ctx.run.jokers).enumerate() {
            let only_weakest = perished.is_some() || (!full && sims < ctx.opts.sims);
            if sj.eternal || (sell.is_none() && others > 0 && cur.key == "j_mr_bones") || sell.is_some_and(|x| x != i) || (sell.is_none() && only_weakest && Some(i) != weakest) {
                continue;
            }
            // You can reorder freely: try the freed slot and the right end (where ×Mult goes);
            // the quick screen only tries the end (Blueprint keeps the slot, next to what it copies).
            let label = format!("replace {}", data.name(&cur.key));
            let rest = without(base, i);
            let end = rest.jokers.len();
            if full || sims >= ctx.opts.sims || j.kind == Kind::Blueprint {
                options.push((label.clone(), with_joker(&rest, j.clone(), i), vec![cur.clone()]));
            }
            if j.kind != Kind::Blueprint && (i != end || !(full || sims >= ctx.opts.sims)) {
                let label = if i != end { format!("{label}, put it rightmost") } else { label };
                options.push((label, with_joker(&rest, j.clone(), end), vec![cur.clone()]));
            }
        }
    }
    // Pick the option with the best typical score, then simulate the rounds for it.
    let mut best: Option<(String, Board, Vec<Joker>, Stats)> = None;
    let survival = |removed: &[Joker]| removed.iter().any(|x| x.key == "j_mr_bones");
    for (label, b, removed) in options {
        let r: Vec<&Joker> = removed.iter().collect();
        let n = if sims >= ctx.opts.sims { ctx.opts.hand_samples } else { SCREEN_DEALS };
        let t = ctx.typical_n(&b, &[&j], &r, n);
        // Equal scores: keep Mr. Bones (his value is surviving, not scoring)
        let better = |bt: &Stats, bremoved: &[Joker]| {
            t.mean > bt.mean * 1.001 || (t.mean >= bt.mean * 0.999 && survival(bremoved) && !survival(&removed))
        };
        if best.as_ref().is_none_or(|(_, _, br, bt)| better(bt, br)) {
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
            desc: None,
            growth: None,
            long_mult: None,
            sell_note: None,
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
        desc: None,
        growth: None,
        long_mult: None,
        sell_note: None,
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

/// Boss effects (texts adapted from balatro-agent's `tools/balatro_state.py` BOSSES).
pub fn boss_effect(key: &str) -> Option<&'static str> {
    Some(match key {
        "bl_hook" => "Discards 2 random held cards after every hand played",
        "bl_ox" => "Playing your most played hand sets money to $0",
        "bl_house" => "First hand is drawn face down",
        "bl_wall" => "Extra large blind (4× base)",
        "bl_wheel" => "1 in 7 cards drawn face down",
        "bl_arm" => "Lowers the level of the hand you play by 1",
        "bl_club" => "All Clubs are debuffed",
        "bl_fish" => "Cards drawn face down after each hand",
        "bl_psychic" => "Must play exactly 5 cards",
        "bl_goad" => "All Spades are debuffed",
        "bl_water" => "Start with 0 discards",
        "bl_window" => "All Diamonds are debuffed",
        "bl_manacle" => "−1 hand size",
        "bl_eye" => "No repeat hand types this round",
        "bl_mouth" => "Only one hand type can be played this round",
        "bl_plant" => "All face cards are debuffed",
        "bl_serpent" => "After a play or discard, always draw exactly 3 cards",
        "bl_pillar" => "Cards played earlier this ante are debuffed",
        "bl_needle" => "Only 1 hand (blind is 1× base)",
        "bl_head" => "All Hearts are debuffed",
        "bl_tooth" => "Lose $1 per card played",
        "bl_flint" => "Base chips and mult are halved",
        "bl_mark" => "All face cards are drawn face down",
        "bl_final_acorn" => "Flips and shuffles all jokers",
        "bl_final_leaf" => "All cards debuffed until 1 joker is sold",
        "bl_final_vessel" => "Very large blind (6× base)",
        "bl_final_heart" => "One random joker disabled every hand",
        "bl_final_bell" => "Forces 1 card to always be selected",
        _ => return None,
    })
}

/// Planets (bought, held or from Celestial packs), Buffoon packs and rerolls, all valued as
/// the win chance for `round` afterwards. Packs and rerolls are expected values: you take
/// the best thing offered, if it beats what you have and you can pay for it.
#[allow(clippy::too_many_arguments)]
fn shop_options(
    ctx: &Ctx,
    run: &RunState,
    data: &GameData,
    pool: &[Candidate],
    base_odds: &[(f64, Stats)],
    round: usize,
    per_rarity: [usize; 4],
    joker_share: f64,
    shop_jokers: &[Candidate],
) -> (Vec<ShopOption>, Vec<TarotValue>) {
    use crate::engine::HandType;
    let now = base_odds.get(round).map_or(0.0, |o| o.0);
    let Some(spec) = ctx.specs.get(round) else { return (vec![], vec![]) };
    let mut out = Vec::new();

    // Planets the game can offer now (Planet X, Ceres, Eris only once their hand was played)
    let planets: Vec<(&crate::data::Center, HandType)> = data
        .centers
        .iter()
        .filter(|c| c.set == "Planet")
        .filter_map(|c| {
            let h = HandType::from_name(c.config.get("hand_type")?.as_str()?)?;
            let softlock = c.config.get("softlock").and_then(|v| v.as_bool()).unwrap_or(false);
            (!softlock || ctx.base.levels[h as usize].played > 0).then_some((c, h))
        })
        .collect();
    let planet_p: Vec<(f64, f64)> = par_map(&planets, |(_, h)| {
        let mut b = ctx.base.clone();
        let l = b.levels[*h as usize];
        b.levels[*h as usize] = l.with_level(l.level + 1);
        let (p, st) = ctx.odds_one(&b, spec, ctx.opts.sims);
        (p, st.mean / spec.start.target.max(1.0))
    });
    let planet_reach: Vec<f64> = planet_p.iter().map(|x| x.1).collect();
    let planet_p: Vec<f64> = planet_p.iter().map(|x| x.0).collect();
    let p_of = |key: &str| planets.iter().position(|(c, _)| c.key == key).map(|i| planet_p[i]);
    let reach_of = |key: &str| planets.iter().position(|(c, _)| c.key == key).map(|i| planet_reach[i]);

    let shop_cards = run.shop.as_ref().map(|s| s.other_cards.clone()).unwrap_or_default();
    for c in shop_cards.iter().filter(|c| c.set == "Planet") {
        if let Some(p) = p_of(&c.key) {
            out.push(ShopOption { reach: None, label: c.name.clone(), kind: "planet".into(), cost: c.cost, money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: 0.0, long_mult: None, key: Some(c.key.clone()), desc: None, p_win: p, note: format!("levels up {}", data.center(&c.key).and_then(|x| x.config.get("hand_type")).and_then(|v| v.as_str()).unwrap_or("")) });
        }
    }
    for c in run.open_pack.iter().filter(|c| c.set == "Planet") {
        if let Some(p) = p_of(&c.key) {
            out.push(ShopOption { reach: None, label: format!("pick {}", c.name), kind: "planet".into(), cost: 0, p_win: p, note: format!("levels up {}", data.center(&c.key).and_then(|x| x.config.get("hand_type")).and_then(|v| v.as_str()).unwrap_or("")), money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: 0.0, long_mult: None, key: Some(c.key.clone()), desc: None });
        }
    }
    for c in run.consumables.iter().filter(|c| c.set == "Planet") {
        if let Some(p) = p_of(&c.key) {
            out.push(ShopOption { reach: None, label: format!("{} (you have it)", c.name), kind: "planet".into(), cost: 0, p_win: p, note: "use it before the blind".into(), money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: 0.0, long_mult: None, key: Some(c.key.clone()), desc: None });
        }
    }

    let mut rng = crate::engine::Rng::new(ctx.opts.seed ^ 0x5eed);
    let packs = run.shop.as_ref().map(|s| s.boosters.clone()).unwrap_or_default();
    for pk in &packs {
        let Some(center) = data.center(&pk.key) else { continue };
        let extra = center.config.get("extra").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        let choose = center.config.get("choose").and_then(|v| v.as_u64()).unwrap_or(1);
        let pick_note = if choose > 1 { " (you pick 2; counted as your best 1)" } else { "" };
        if pk.key.starts_with("p_celestial") && !planets.is_empty() {
            // Exact: average over every set of `extra` distinct planets of the best one in it.
            let n = planets.len();
            let k = extra.min(n);
            let (mut sum, mut count): (f64, f64) = (0.0, 0.0);
            let mut best_counts = vec![0usize; n];
            let mut idx: Vec<usize> = (0..k).collect();
            loop {
                let bi = *idx.iter().max_by(|&&a, &&b| planet_p[a].total_cmp(&planet_p[b])).unwrap();
                sum += planet_p[bi].max(now);
                best_counts[bi] += 1;
                count += 1.0;
                // next combination
                let mut i = k;
                while i > 0 && idx[i - 1] == n - k + i - 1 {
                    i -= 1;
                }
                if i == 0 {
                    break;
                }
                idx[i - 1] += 1;
                for j in i..k {
                    idx[j] = idx[j - 1] + 1;
                }
            }
            let top = (0..n).max_by_key(|&i| best_counts[i]).unwrap_or(0);
            out.push(ShopOption { reach: None,
                money_after: 0.0,
                interest_now: 0,
                interest_after: 0,
                unaffordable: false,
                money_gain: 0.0,
                long_mult: None,
                key: None,
                desc: None,
                label: pk.name.clone(),
                kind: "pack".into(),
                cost: pk.cost,
                p_win: sum / count.max(1.0),
                note: format!("{extra} planets, best is usually {} ({:.0}% of packs){pick_note}", planets[top].0.name, best_counts[top] as f64 * 100.0 / count.max(1.0)),
            });
        } else if pk.key.starts_with("p_buffoon") {
            // Jokers picked from a pack are free: no budget limit on what's inside.
            let e = expected_best(pool, round, now, extra, 1.0, f64::INFINITY, per_rarity, &mut rng);
            out.push(ShopOption { reach: None, label: pk.name.clone(), kind: "pack".into(), cost: pk.cost, p_win: e, note: format!("{extra} jokers{pick_note}"), money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: 0.0, long_mult: None, key: None, desc: None });
        }
    }

    // Rerolls: each redraws the shop's card slots (not packs or vouchers).
    if let Some(shop) = &run.shop {
        let slots = run.shop_rates.slots.max(1) as usize;
        let mut spent = 0i64;
        // Only the next reroll: the advisor re-evaluates after each one anyway.
        for k in 1..=1usize {
            spent += shop.reroll_cost + k as i64 - 1;
            if spent as f64 > run.dollars {
                break;
            }
            let budget = run.dollars - spent as f64;
            let e = expected_best(pool, round, now, slots * k, joker_share, budget, per_rarity, &mut rng);
            out.push(ShopOption { reach: None,
                money_after: 0.0,
                interest_now: 0,
                interest_after: 0,
                unaffordable: false,
                money_gain: 0.0,
                long_mult: None,
                key: None,
                desc: None,
                label: "reroll".to_string(),
                kind: "reroll".into(),
                cost: spent,
                p_win: e,
                note: format!("{} new cards; buy the best joker if it helps and fits the ${budget:.0} left", slots * k),
            });
        }
    }
    // Vouchers: the ones that change a round are re-simulated; economy ones get exact notes.
    if let Some(shop) = &run.shop {
        for v in shop.vouchers.iter().filter(|v| matches!(v.key.as_str(), "v_directors_cut" | "v_retcon")) {
            let mut o = boss_reroll_option(ctx, run, data, spec, now);
            o.label = v.name.clone();
            o.cost = v.cost;
            o.note = format!("{}{}", if v.key == "v_retcon" { "reroll the boss for $10, as often as you like" } else { "reroll the boss once per ante for $10" }, o.note);
            out.push(o);
        }
        for v in shop.vouchers.iter().filter(|v| !matches!(v.key.as_str(), "v_directors_cut" | "v_retcon")) {
            let mut sp = spec.clone();
            let (sim, note): (bool, String) = match v.key.as_str() {
                "v_grabber" | "v_nacho_tong" => {
                    sp.start.hands += 1;
                    (true, "+1 hand every round".into())
                }
                "v_wasteful" | "v_recyclomancy" => {
                    sp.start.discards += 1;
                    (true, "+1 discard every round".into())
                }
                "v_paint_brush" | "v_palette" => {
                    sp.start.hand_size += 1;
                    (true, "+1 hand size".into())
                }
                "v_antimatter" => (false, "+1 joker slot: add jokers instead of selling one".into()),
                "v_seed_money" => (false, format!("interest cap ${} → $10 per round", run.interest_cap / 5)),
                "v_money_tree" => (false, format!("interest cap ${} → $20 per round", run.interest_cap / 5)),
                "v_overstock_norm" | "v_overstock_plus" => (false, format!("+1 shop card slot ({} → {} per shop and reroll)", run.shop_rates.slots, run.shop_rates.slots + 1)),
                "v_reroll_surplus" | "v_reroll_glut" => (false, "rerolls cost $2 less".into()),
                "v_clearance_sale" => (false, "everything in the shop 25% off".into()),
                "v_liquidation" => (false, "everything in the shop 50% off".into()),
                "v_hieroglyph" => (false, "−1 ante, but −1 hand every round".into()),
                "v_petroglyph" => (false, "−1 ante, but −1 discard every round".into()),
                "v_crystal_ball" => (false, "+1 consumable slot".into()),
                "v_tarot_merchant" | "v_tarot_tycoon" => (false, "more tarots in the shop, so fewer jokers per slot".into()),
                "v_planet_merchant" | "v_planet_tycoon" => (false, "more planets in the shop, so fewer jokers per slot".into()),
                "v_observatory" => (false, "planets you hold give ×1.5 mult for their hand".into()),
                "v_telescope" => (false, "Celestial packs always contain your most played hand's planet".into()),
                _ => (false, "not valued".into()),
            };
            let p = if sim { ctx.odds_one(&ctx.base, &sp, ctx.opts.sims).0 } else { now };
            out.push(ShopOption { reach: None, label: v.name.clone(), kind: "voucher".into(), cost: v.cost, p_win: p, note, money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: 0.0, long_mult: None, key: None, desc: None });
        }
    }
    // Tarots: applied to the deck the way a player sensibly would, then the round re-simulated.
    let mut tarots = tarot_values(ctx, run, data, spec, round, now, pool, per_rarity, &planet_p);
    // The Fool makes a copy of the last tarot or planet used: worth what that one is worth.
    if let Some(last) = &run.last_tarot_planet {
        let copied = p_of(last)
            .map(|p| (p, 0.0))
            .or_else(|| tarots.iter().find(|t| &t.key == last).map(|t| (t.p_win, t.money_gain)));
        if let (Some((p, gain)), Some(fool)) = (copied, tarots.iter_mut().find(|t| t.key == "c_fool")) {
            fool.p_win = p;
            fool.money_gain = gain;
            fool.simulated = true;
            // It needs a free consumable slot for the copy; using another card first changes the copy.
            let full = run.consumables.len() as i64 >= run.consumable_slots;
            fool.note = format!(
                "makes {} (the last one used){}",
                data.name(last),
                if full { "; needs a free consumable slot, so sell one first (using one would change what it copies)" } else { "; use it before another tarot or planet, or it copies that one" }
            );
        }
    }
    let tarot_p = |key: &str| tarots.iter().find(|t| t.key == key);
    let shop_cards = run.shop.as_ref().map(|s| s.other_cards.clone()).unwrap_or_default();
    for c in shop_cards.iter().filter(|c| c.set == "Tarot") {
        if let Some(t) = tarot_p(&c.key) {
            out.push(ShopOption { reach: None, label: c.name.clone(), kind: "tarot".into(), cost: c.cost, p_win: t.p_win, note: t.note.clone(), money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: t.money_gain, long_mult: None, key: Some(c.key.clone()), desc: None });
        }
    }
    for c in run.open_pack.iter().filter(|c| c.set == "Tarot") {
        if let Some(t) = tarot_p(&c.key) {
            out.push(ShopOption { reach: None, label: format!("pick {}", c.name), kind: "tarot".into(), cost: 0, p_win: t.p_win, note: format!("{} · next ante reach {:.0}% → {:.0}%", t.note, t.reach_now * 100.0, t.reach * 100.0), money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: t.money_gain, long_mult: None, key: Some(c.key.clone()), desc: None });
        }
    }
    for c in run.consumables.iter().filter(|c| c.set == "Tarot") {
        if let Some(t) = tarot_p(&c.key) {
            out.push(ShopOption { reach: None, label: format!("{} (you have it)", c.name), kind: "tarot".into(), cost: 0, p_win: t.p_win, note: format!("{} · use it during a blind", t.note), money_after: 0.0, interest_now: 0, interest_after: 0, unaffordable: false, money_gain: t.money_gain, long_mult: None, key: Some(c.key.clone()), desc: None });
        }
    }
    for pk in run.shop.as_ref().map(|s| s.boosters.clone()).unwrap_or_default().iter().filter(|p| p.key.starts_with("p_arcana")) {
        let Some(center) = data.center(&pk.key) else { continue };
        let extra = center.config.get("extra").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        let choose = center.config.get("choose").and_then(|v| v.as_u64()).unwrap_or(1);
        let vals: Vec<f64> = tarots.iter().map(|t| t.p_win.max(now)).collect();
        let (avg, top) = best_of_subsets(&vals, extra);
        out.push(ShopOption { reach: None,
            label: pk.name.clone(),
            kind: "pack".into(),
            cost: pk.cost,
            p_win: avg,
            note: format!("{extra} tarots, best is usually {}{}", tarots.get(top).map_or("?", |t| t.name.as_str()), if choose > 1 { " (you pick 2; counted as your best 1)" } else { "" }),
            money_after: 0.0,
            interest_now: 0,
            interest_after: 0,
            unaffordable: false,
            money_gain: 0.0,
            long_mult: None,
            key: None,
            desc: None,
        });
    }

    // Jokers in the shop (and an open Buffoon pack) are options like any other.
    for c in shop_jokers {
        // Slots are full: "sell X for it" (and where to put it)
        let action = match c.action.strip_prefix("replace ") {
            Some(rest) => match rest.split_once(", ") {
                Some((name, place)) => format!("sell {name} for it, {place}"),
                None => format!("sell {rest} for it"),
            },
            None => String::new(),
        };
        let later = c.reach.iter().zip(&c.reach_delta).zip(&ctx.specs).find(|(_, sp)| sp.horizon).map(|((r, d), sp)| {
            format!("{} reach {:.0}% → {:.0}%", sp.label, (r - d) * 100.0, r * 100.0)
        });
        let stickers = run.shop.as_ref().and_then(|sh| sh.jokers.iter().find(|j| j.key == c.key)).map(|j| {
            let mut v = Vec::new();
            if let Some(n) = j.perishable {
                v.push(format!("perishable: {n} rounds"));
            }
            if j.rental {
                v.push("rental $3/round".into());
            }
            if j.eternal {
                v.push("eternal: can't sell".into());
            }
            v.join(", ")
        }).filter(|s| !s.is_empty());
        let action = c.sell_note.clone().unwrap_or(action);
        let note = [stickers, Some(action).filter(|a| !a.is_empty()), later].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let from_pack = run.open_pack.iter().any(|p| p.key == c.key);
        out.push(ShopOption { reach: None,
            label: if from_pack { format!("pick {}", c.name) } else { c.name.clone() },
            kind: "joker".into(),
            cost: c.cost,
            p_win: c.p_win.get(round).copied().unwrap_or(now),
            note,
            money_after: 0.0,
            interest_now: 0,
            interest_after: 0,
            unaffordable: false,
            money_gain: 0.0,
            long_mult: c.long_mult,
            key: Some(c.key.clone()),
            desc: c.desc.clone(),
        });
    }
    let base_reach = base_odds.get(round).map_or(0.0, |o| o.1.mean / spec.start.target.max(1.0));
    for o in &mut out {
        if o.kind == "planet" {
            o.reach = o.key.as_deref().and_then(reach_of);
        } else if o.kind == "joker" {
            o.reach = o.key.as_ref().and_then(|k| shop_jokers.iter().find(|c| &c.key == k)).and_then(|c| c.reach.get(round).copied());
        }
        o.unaffordable = o.cost as f64 > run.dollars;
        o.money_after = run.dollars - o.cost as f64 + o.money_gain;
        o.interest_now = interest(run.dollars, run.interest_amount, run.interest_cap);
        o.interest_after = interest(o.money_after, run.interest_amount, run.interest_cap);
    }
    rank_options(&mut out, base_reach);
    (out, tarots)
}

/// Average over random draws of the best By Ante 8 value among `cards` shop cards (each a
/// joker with chance `joker_share`, rarity 70/25/5), never below ×1.00 (a bad one is sold).
fn long_draw(pool: &[Candidate], cards: usize, joker_share: f64, rng: &mut crate::engine::Rng) -> f64 {
    use crate::engine::Rolls;
    let by_rarity: [Vec<f64>; 4] = [0u8, 1, 2, 3].map(|r| pool.iter().filter(|c| c.rarity_n == r).map(|c| c.long_mult.unwrap_or(1.0)).collect());
    let trials = 2000;
    let mut total = 0.0;
    for _ in 0..trials {
        let mut best: f64 = 1.0;
        for _ in 0..cards {
            if !rng.chance(joker_share) {
                continue;
            }
            let roll = rng.unit();
            let list = &by_rarity[if roll > 0.95 { 3 } else if roll > 0.7 { 2 } else { 1 }];
            if !list.is_empty() {
                best = best.max(list[rng.below(list.len())]);
            }
        }
        total += best;
    }
    total / trials as f64
}

/// Director's Cut / Retcon: the options round's boss against a reroll into any boss the
/// game could draw instead (`get_new_boss` in common_events.lua: bosses allowed at this
/// ante, showdown ones only on the final ante, banned ones out, least drawn first).
/// Rerolling costs $10, counted in the money after when it's worth doing.
fn boss_reroll_option(ctx: &Ctx, run: &RunState, data: &GameData, spec: &Spec, now: f64) -> ShopOption {
    let mut o = ShopOption {
        reach: None,
        label: String::new(),
        kind: "voucher".into(),
        cost: 0,
        p_win: now,
        note: String::new(),
        money_after: 0.0,
        interest_now: 0,
        interest_after: 0,
        unaffordable: false,
        money_gain: 0.0,
        long_mult: None,
        key: None,
        desc: None,
    };
    let Some(cur) = data.blinds.iter().find(|b| b.key == spec.blind_key && !b.boss.is_null()) else {
        o.note = " (no boss left this ante to reroll)".into();
        return o;
    };
    let showdown = run.ante >= 2 && run.ante % run.win_ante.max(1) == 0;
    let used = |k: &str| run.bosses_used.iter().find(|(x, _)| x == k).map_or(0, |x| x.1);
    let eligible: Vec<&crate::data::Blind> = data
        .blinds
        .iter()
        .filter(|b| b.key != cur.key && !run.banned_keys.contains(&b.key))
        .filter(|b| {
            let sd = b.boss.get("showdown").and_then(|v| v.as_bool()).unwrap_or(false);
            let min = b.boss.get("min").and_then(|v| v.as_i64());
            min.is_some() && if showdown { sd } else { !sd && min.unwrap_or(99) <= run.ante.max(1) }
        })
        .collect();
    let least = eligible.iter().map(|b| used(&b.key)).min().unwrap_or(0);
    let eligible: Vec<&crate::data::Blind> = eligible.into_iter().filter(|b| used(&b.key) == least).collect();
    if eligible.is_empty() {
        o.note = " (no other boss to reroll into)".into();
        return o;
    }
    let base_target = spec.start.target / cur.mult.max(0.1);
    let odds: Vec<f64> = par_map(&eligible, |b| {
        let mut sp = spec.clone();
        sp.blind_key = b.key.clone();
        sp.blind_name = b.name.clone();
        sp.rules = RoundRules::for_blind(&b.key);
        sp.start.target = base_target * b.mult;
        sp.start.hand_size = run.hand_size + sp.rules.hand_size_delta;
        sp.start.hands = if b.key == "bl_needle" { 1 } else { run.round_hands };
        sp.start.discards = if b.key == "bl_water" { 0 } else { run.round_discards };
        ctx.odds_one(&ctx.base, &sp, (ctx.opts.sims / 2).max(50)).0
    });
    let avg = odds.iter().sum::<f64>() / odds.len() as f64;
    let rough = eligible.iter().filter(|b| unmodelled_boss(&b.key).is_some()).count();
    let rough_note = if rough > 0 { format!("; {rough} of them have effects that aren't simulated, so the reroll may look better than it is") } else { String::new() };
    if avg > now {
        o.p_win = avg;
        o.money_gain = -10.0;
        o.note = format!(": worth using on {} now ({:.0}% → {:.0}% on average over {} bosses it could become{rough_note})", cur.name, now * 100.0, avg * 100.0, eligible.len());
    } else {
        o.note = format!(": keep {} ({:.0}%; a reroll averages {:.0}% over {} bosses{rough_note}). Also a reroll on every later boss, Ante 8's included (not valued)", cur.name, now * 100.0, avg * 100.0, eligible.len());
    }
    o
}

/// Win chance first, in 2-point tiers counted down from the best option (closer than
/// that is simulation noise); within a tier the long run (By Ante 8) in 0.05 tiers from
/// the tier's best, for the same reason; then the score reached this round.
fn rank_options(out: &mut [ShopOption], base_reach: f64) {
    let top = out.iter().map(|o| o.p_win).fold(0.0, f64::max);
    let p_tier = |o: &ShopOption| ((top - o.p_win) / 0.02).floor() as i64;
    let long = |o: &ShopOption| o.long_mult.unwrap_or(1.0);
    let tops: std::collections::HashMap<i64, f64> = out.iter().fold(Default::default(), |mut m, o| {
        let e = m.entry(p_tier(o)).or_insert(f64::MIN);
        *e = e.max(long(o));
        m
    });
    let key = |o: &ShopOption| {
        let t = p_tier(o);
        (t, ((tops[&t] - long(o)) / 0.05).floor() as i64, o.reach.unwrap_or(base_reach))
    };
    out.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.cmp(&kb.0).then(ka.1.cmp(&kb.1)).then(kb.2.total_cmp(&ka.2))
    });
}

/// Average over every `k`-subset of `vals` of its maximum, and the index most often best.
fn best_of_subsets(vals: &[f64], k: usize) -> (f64, usize) {
    let n = vals.len();
    let k = k.min(n);
    if k == 0 {
        return (0.0, 0);
    }
    let (mut sum, mut count) = (0.0f64, 0.0f64);
    let mut wins = vec![0usize; n];
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        let bi = *idx.iter().max_by(|&&a, &&b| vals[a].total_cmp(&vals[b])).unwrap();
        sum += vals[bi];
        wins[bi] += 1;
        count += 1.0;
        let mut i = k;
        while i > 0 && idx[i - 1] == n - k + i - 1 {
            i -= 1;
        }
        if i == 0 {
            break;
        }
        idx[i - 1] += 1;
        for j in i..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
    (sum / count.max(1.0), (0..n).max_by_key(|&i| wins[i]).unwrap_or(0))
}

/// Values every tarot for this run. Card-targeting ones pick targets the way a player
/// sensibly would (heuristic, and in the game only among the cards in hand).
#[allow(clippy::too_many_arguments)]
fn tarot_values(
    ctx: &Ctx,
    run: &RunState,
    data: &GameData,
    spec: &Spec,
    round: usize,
    now: f64,
    pool: &[Candidate],
    per_rarity: [usize; 4],
    planet_p: &[f64],
) -> Vec<TarotValue> {
    use crate::model::{Enhancement, Rank, Suit};
    let tarots: Vec<&crate::data::Center> = data.centers.iter().filter(|c| c.set == "Tarot").collect();
    let total_rate = run.shop_rates.joker + run.shop_rates.tarot + run.shop_rates.planet + run.shop_rates.spectral + run.shop_rates.playing_card;
    let per_card = if total_rate > 0.0 { run.shop_rates.tarot / total_rate / tarots.len().max(1) as f64 } else { 0.0 };
    let per_shop = 1.0 - (1.0 - per_card).powi(run.shop_rates.slots.max(1) as i32);

    // A fresh version of the round to re-simulate with the changed deck
    let mut fresh = spec.clone();
    if fresh.in_progress {
        fresh.in_progress = false;
        fresh.start.hand.clear();
        fresh.start.scored = 0.0;
        fresh.start.hands = run.round_hands;
        fresh.start.discards = run.round_discards;
    }
    fresh.start.deck = ctx.fresh_deck.clone();
    let deck = &ctx.fresh_deck;
    let count = |s: Suit| deck.iter().filter(|c| c.suit == s && c.enhancement != Some(Enhancement::Stone)).count();
    // Your main suit, only if one suit clearly leads (a tie means there isn't one)
    let mut by_count: Vec<(usize, Suit)> = Suit::ALL.iter().map(|&s| (count(s), s)).collect();
    by_count.sort_by(|a, b| b.0.cmp(&a.0));
    let main: Option<Suit> = (by_count[0].0 > by_count[1].0).then_some(by_count[0].1);
    let is_main = |c: &Card| main.is_none_or(|m| c.suit == m);
    let even = if main.is_none() { " (your suits are even)" } else { "" };
    let main_name = main.map_or("card".to_string(), |m| m.name().trim_end_matches('s').to_string());
    let weakest_suit = Suit::ALL.into_iter().filter(|&s| Some(s) != main).min_by_key(|&s| count(s)).unwrap_or(Suit::Clubs);
    // Card-targeting tarots only reach the cards in hand. When a hand is on screen (a blind or
    // an opened pack), targets are limited to those cards; otherwise any card is assumed.
    let mut in_hand = vec![run.hand.is_empty(); deck.len()];
    if !run.hand.is_empty() {
        for h in &run.hand {
            if let Some(i) = (0..deck.len()).find(|&i| !in_hand[i] && deck[i].rank == h.rank && deck[i].suit == h.suit && deck[i].enhancement == h.enhancement) {
                in_hand[i] = true;
            }
        }
    }
    let from_hand = !run.hand.is_empty();
    // Indices of plain cards (in hand, when one is shown), best (high rank) first / worst first
    let plain = |pred: &dyn Fn(&Card) -> bool, best_first: bool| -> Vec<usize> {
        let mut v: Vec<usize> = (0..deck.len()).filter(|&i| in_hand[i] && deck[i].enhancement.is_none() && pred(&deck[i])).collect();
        v.sort_by_key(|&i| deck[i].rank.0);
        if best_first {
            v.reverse();
        }
        v
    };

    let horizon = ctx.specs.iter().find(|x| x.horizon).cloned();
    let sims = (ctx.opts.sims / 2).max(100);
    let board_for_deck = |d: &[Card]| {
        let mut b = ctx.base.clone();
        let tally = |e: Enhancement| d.iter().filter(|c| c.enhancement == Some(e)).count() as i64;
        b.steel_tally = tally(Enhancement::Steel);
        b.stone_tally = tally(Enhancement::Stone);
        b.driver_tally = d.iter().filter(|c| c.enhancement.is_some()).count() as i64;
        b.playing_cards = d.len() as i64;
        b
    };
    let reach_of = |d: &[Card]| -> f64 {
        let Some(h) = &horizon else { return 0.0 };
        let mut sp = h.clone();
        sp.start.deck = d.to_vec();
        ctx.odds_one(&board_for_deck(d), &sp, sims).1.mean / sp.start.target.max(1.0)
    };
    let reach_now = reach_of(deck);
    let simulate = |d: Vec<Card>| -> (f64, f64) {
        let mut sp = fresh.clone();
        sp.start.deck = d.clone();
        (ctx.odds_one(&board_for_deck(&d), &sp, sims).0, reach_of(&d))
    };

    let hz_idx = ctx.specs.iter().position(|x| x.horizon);
    let money_gain = |key: &str| -> Option<f64> {
        match key {
            "c_hermit" => Some(run.dollars.clamp(0.0, 20.0)),
            "c_temperance" => Some(run.jokers.iter().map(|j| j.sell_value).sum::<i64>().min(50) as f64),
            _ => None,
        }
    };
    let mut out = par_map(&tarots, |t| {
        let cfg = &t.config;
        let n = cfg.get("max_highlighted").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let mut d = deck.clone();
        let (sim, note): (bool, String) = if let Some(suit) = cfg.get("suit_conv").and_then(|v| v.as_str()).and_then(Suit::from_name) {
            // 3 cards of your least-used suit (lowest first) become this suit
            // Take from your least-used suit (or the next one, if that's the target suit)
            let source = Suit::ALL.into_iter().filter(|&x| x != suit && Some(x) != main).min_by_key(|&x| count(x))
                .or_else(|| Suit::ALL.into_iter().filter(|&x| x != suit).min_by_key(|&x| count(x)))
                .unwrap_or(weakest_suit);
            for &i in plain(&|c: &Card| c.suit == source, false).iter().take(n) {
                d[i].suit = suit;
            }
            (true, format!("{n} {} → {}{even}", source.name(), suit.name()))
        } else if let Some(m) = cfg.get("mod_conv").and_then(|v| v.as_str()) {
            match m {
                "up_rank" => {
                    for &i in plain(&is_main, false).iter().take(n) {
                        d[i].rank = Rank(if d[i].rank.0 >= 14 { 2 } else { d[i].rank.0 + 1 });
                    }
                    (true, format!("+1 rank on your {n} lowest {main_name}s{even}"))
                }
                "card" => {
                    // Death: the worst card becomes a copy of the best one
                    let best = plain(&is_main, true).first().copied();
                    let worst = plain(&|c: &Card| c.suit == weakest_suit, false).first().copied();
                    if let (Some(b), Some(w)) = (best, worst) {
                        d[w] = d[b];
                    }
                    (true, format!("a low {} becomes a copy of your best {main_name}{even}", weakest_suit.name().trim_end_matches('s')))
                }
                "m_gold" => (false, "Gold card: $3 per round while held (economy)".into()),
                _ => {
                    let e = Enhancement::from_key(m);
                    // Steel and Stone go on cards you'd hold or throw in; the rest on your main suit's high cards
                    let targets = match e {
                        Some(Enhancement::Steel) => plain(&|c: &Card| main.is_none_or(|m| c.suit != m), true),
                        Some(Enhancement::Stone) => plain(&|c: &Card| c.suit == weakest_suit, false),
                        _ => plain(&is_main, true),
                    };
                    let names: Vec<String> = targets.iter().take(n).map(|&i| d[i].label()).collect();
                    for &i in targets.iter().take(n) {
                        d[i].enhancement = e;
                    }
                    let on = if from_hand && !names.is_empty() { names.join(" ") } else { format!("{n} card{}", if n > 1 { "s" } else { "" }) };
                    (true, format!("{} on {on}", m.trim_start_matches("m_")))
                }
            }
        } else if cfg.get("remove_card").and_then(|v| v.as_bool()).unwrap_or(false) {
            for i in plain(&|c: &Card| c.suit == weakest_suit, false).into_iter().take(n).collect::<Vec<_>>().into_iter().rev() {
                d.remove(i);
            }
            (true, format!("destroys {n} low {}", weakest_suit.name()))
        } else {
            let note = match t.key.as_str() {
                "c_hermit" => format!("doubles money: +${:.0}", run.dollars.clamp(0.0, 20.0)),
                "c_temperance" => {
                    let v: i64 = run.jokers.iter().map(|j| j.sell_value).sum();
                    format!("+${} (your jokers' sell value, max $50)", v.min(50))
                }
                "c_judgement" if run.jokers.len() as i64 >= run.joker_slots => match run.jokers.iter().find(|j| j.perishable == Some(0)) {
                    Some(dead) => format!("creates a random joker; needs a free joker slot: sell {} first (perished)", data.name(&dead.key)),
                    None => "creates a random joker; needs a free joker slot, so sell one first".into(),
                },
                "c_judgement" => "creates a random joker".into(),
                "c_high_priestess" => "creates 2 random planets".into(),
                "c_emperor" => "creates 2 random tarots".into(),
                "c_fool" => "copies the last tarot or planet used".into(),
                "c_wheel_of_fortune" => "1 in 4: a random joker gets Foil/Holo/Polychrome".into(),
                _ => "not valued".into(),
            };
            let p = match t.key.as_str() {
                "c_judgement" => {
                    let mut rng = crate::engine::Rng::new(ctx.opts.seed ^ 0x7a70);
                    let reach = pool_reach(pool, 1, &mut rng).unwrap_or(reach_now).max(reach_now);
                    return TarotValue { deck: None, key: t.key.clone(), name: t.name.clone(), p_win: expected_best(pool, round, now, 1, 1.0, f64::INFINITY, per_rarity, &mut rng).max(now), note, simulated: true, per_shop, reach, reach_now, money_gain: 0.0 };
                }
                "c_wheel_of_fortune" => {
                    // card.lua: 1 in 4 hits a random joker without an edition; the edition is
                    // poll_edition(guaranteed, no negative): Polychrome 15%, Holo 35%, Foil 50%.
                    let plain: Vec<usize> = ctx.base.jokers.iter().enumerate().filter(|(_, j)| j.edition.is_none()).map(|(i, _)| i).collect();
                    if plain.is_empty() {
                        return TarotValue { deck: None, key: t.key.clone(), name: t.name.clone(), p_win: now, note: "no joker without an edition to hit".into(), simulated: true, per_shop, reach: reach_now, reach_now, money_gain: 0.0 };
                    }
                    let hit = (run.probability_normal / 4.0).min(1.0);
                    let (mut p, mut r) = (0.0, 0.0);
                    for &i in &plain {
                        for (e, w) in [(Edition::Polychrome, 0.15), (Edition::Holo, 0.35), (Edition::Foil, 0.5)] {
                            let mut b = board_for_deck(deck);
                            b.jokers[i].edition = Some(e);
                            p += w * ctx.odds_one(&b, &fresh, sims).0;
                            if let Some(h) = &horizon {
                                r += w * ctx.odds_one(&b, h, sims).1.mean / h.start.target.max(1.0);
                            }
                        }
                    }
                    let k = plain.len() as f64;
                    let p = (1.0 - hit) * now + hit * p / k;
                    let reach = (1.0 - hit) * reach_now + hit * r / k;
                    let note = format!("{:.0}% chance: one of your {} jokers without an edition gets Polychrome 15% / Holo 35% / Foil 50%", hit * 100.0, plain.len());
                    return TarotValue { deck: None, key: t.key.clone(), name: t.name.clone(), p_win: p, note, simulated: true, per_shop, reach, reach_now, money_gain: 0.0 };
                }
                "c_high_priestess" if !planet_p.is_empty() => best_of_subsets(planet_p, 2).0.max(now),
                _ => now,
            };
            return TarotValue { deck: None, key: t.key.clone(), name: t.name.clone(), p_win: p, note, simulated: p != now, per_shop, reach: reach_now, reach_now, money_gain: 0.0 };
        };
        let changed = sim.then(|| d.clone());
        let (p, reach) = if sim { simulate(d) } else { (now, reach_now) };
        // Card-targeting tarots only work on cards in hand (in a blind or an opened pack)
        let note = if n > 0 && !from_hand { format!("{note}, if you hold suitable cards") } else if n > 0 { format!("{note} (from your hand)") } else { note };
        TarotValue { deck: changed, key: t.key.clone(), name: t.name.clone(), p_win: p, note, simulated: sim, per_shop, reach, reach_now, money_gain: 0.0 }
    });
    // Money tarots: valued by what the extra money buys (rerolls, then the best joker found)
    for t in &mut out {
        let Some(g) = money_gain(&t.key) else { continue };
        if g <= 0.0 {
            continue;
        }
        t.money_gain = g;
        let mut how = String::new();
        if let Some(h) = hz_idx {
            let v = |c: &Candidate| c.reach.get(h).copied().unwrap_or(0.0);
            let (before, after) = (money_value(ctx, pool, &v, reach_now, run.dollars, true), money_value(ctx, pool, &v, reach_now, run.dollars + g, true));
            t.reach = reach_now + (after - before).max(0.0);
            how = format!(
                "the extra ${g:.0} adds about {:+.0} points of next-ante reach (your ${:.0} already buys you to ~{:.0}% over the coming shops; ${:.0} → ~{:.0}%)",
                (after - before) * 100.0, run.dollars, before * 100.0, run.dollars + g, after * 100.0
            );
        }
        let pv = |c: &Candidate| c.p_win.get(round).copied().unwrap_or(0.0);
        t.p_win = now + (money_value(ctx, pool, &pv, now, run.dollars + g, false) - money_value(ctx, pool, &pv, now, run.dollars, false)).max(0.0);
        t.simulated = true;
        t.note = if how.is_empty() { t.note.clone() } else { format!("{} · {how}", t.note) };
    }
    out
}

/// Expected next-ante reach from `cards` free random jokers (the horizon column of the pool).
fn pool_reach(pool: &[Candidate], cards: usize, rng: &mut crate::engine::Rng) -> Option<f64> {
    use crate::engine::Rolls;
    let hz = |c: &Candidate| c.reach.last().copied();
    let by: [Vec<f64>; 4] = [0u8, 1, 2, 3].map(|r| pool.iter().filter(|c| c.rarity_n == r).filter_map(hz).collect());
    let trials = 2000;
    let mut total = 0.0;
    for _ in 0..trials {
        let mut best: f64 = 0.0;
        for _ in 0..cards {
            let u = rng.unit();
            let r = if u > 0.95 { 3 } else if u > 0.7 { 2 } else { 1 };
            if !by[r].is_empty() {
                best = best.max(by[r][rng.below(by[r].len())]);
            }
        }
        total += best;
    }
    Some(total / trials as f64)
}

/// Expected win chance after seeing `cards` random shop/pack cards (each a joker with
/// probability `joker_share`), buying the best affordable joker if it beats `now`.
#[allow(clippy::too_many_arguments)]
fn expected_best(
    pool: &[Candidate],
    round: usize,
    now: f64,
    cards: usize,
    joker_share: f64,
    budget: f64,
    per_rarity: [usize; 4],
    rng: &mut crate::engine::Rng,
) -> f64 {
    use crate::engine::Rolls;
    let by_rarity: [Vec<&Candidate>; 4] = [0u8, 1, 2, 3].map(|r| pool.iter().filter(|c| c.rarity_n == r).collect());
    let trials = 4000;
    let mut total = 0.0;
    for _ in 0..trials {
        let mut best = now;
        for _ in 0..cards {
            if !rng.chance(joker_share) {
                continue;
            }
            let roll = rng.unit();
            let r = if roll > 0.95 { 3 } else if roll > 0.7 { 2 } else { 1 };
            let list = &by_rarity[r];
            if list.is_empty() || per_rarity[r] == 0 {
                continue;
            }
            let c = list[rng.below(list.len())];
            if (c.cost as f64) <= budget {
                best = best.max(c.p_win.get(round).copied().unwrap_or(0.0));
            }
        }
        total += best;
    }
    total / trials as f64
}

/// Shops still to come before a target round: the current one (if you're in it) plus one
/// after each blind before it. `next_ante` adds the shop after this boss and next ante's two.
fn shops_ahead(run: &RunState, next_ante: bool) -> (bool, usize) {
    let in_shop = matches!(run.screen, crate::save::Screen::Shop) || run.screen.in_pack();
    let before_boss = run.blinds.iter().filter(|b| b.slot != "Boss" && matches!(b.state.as_str(), "Select" | "Upcoming" | "Current")).count();
    (in_shop, before_boss + if next_ante { 3 } else { 0 })
}

/// What `money` is worth in `value` terms: the best expected result from spending it on
/// the cheapest rerolls across the shops still to come (each shop's reroll price starts
/// low again) plus their free cards, then on the best joker found. Income between shops
/// (blind reward, a hand's cash, interest) is added. Same seed on every call, so two
/// amounts are compared on the same shops.
fn money_value(ctx: &Ctx, pool: &[Candidate], value: &dyn Fn(&Candidate) -> f64, now: f64, money: f64, next_ante: bool) -> f64 {
    let run = ctx.run;
    let slots = run.shop_rates.slots.max(1) as usize;
    let share = run.shop_rates.joker_share();
    let (in_shop, future) = shops_ahead(run, next_ante);
    // Every reroll you could buy, cheapest first
    let mut costs: Vec<i64> = Vec::new();
    if in_shop {
        let c0 = run.shop.as_ref().map_or(run.base_reroll_cost, |s| s.reroll_cost);
        costs.extend((0..8).map(|i| c0 + i));
    }
    for _ in 0..future {
        costs.extend((0..8).map(|i| run.base_reroll_cost + i));
    }
    costs.sort_unstable();
    let avg_reward = run.blinds.iter().map(|b| b.reward).sum::<i64>() as f64 / run.blinds.len().max(1) as f64;
    let income = future as f64 * (avg_reward + run.money_per_hand + interest(money, run.interest_amount, run.interest_cap) as f64);
    let budget = money + income;
    // Chaos the Clown: one free reroll in every shop
    let chaos = run.jokers.iter().any(|j| j.key == "j_chaos") as usize;
    let free_cards = slots * (future + chaos * (future + in_shop as usize));
    let open_slots = (run.joker_slots - run.jokers.len() as i64).max(1) as usize;
    let mut best = now;
    let mut spent = 0i64;
    for k in 0..=costs.len().min(16) {
        if k > 0 {
            spent += costs[k - 1];
        }
        if spent as f64 > budget {
            break;
        }
        let mut rng = crate::engine::Rng::new(ctx.opts.seed ^ 0x6d6f6e6579 ^ k as u64);
        best = best.max(expected_buys(pool, value, now, free_cards + slots * k, share, budget - spent as f64, open_slots, &mut rng));
    }
    best
}

/// Like `expected_best_by`, but keeps buying: the best jokers seen, while money and free
/// slots last. Later buys count for less (×0.6 each), since joker gains don't simply add
/// up. A heuristic for "what money is worth", not a simulation of the board.
#[allow(clippy::too_many_arguments)]
fn expected_buys(
    pool: &[Candidate],
    value: &dyn Fn(&Candidate) -> f64,
    now: f64,
    cards: usize,
    joker_share: f64,
    budget: f64,
    open_slots: usize,
    rng: &mut crate::engine::Rng,
) -> f64 {
    use crate::engine::Rolls;
    let by_rarity: [Vec<&Candidate>; 4] = [0u8, 1, 2, 3].map(|r| pool.iter().filter(|c| c.rarity_n == r).collect());
    let trials = 2000;
    let mut total = 0.0;
    let mut seen: Vec<(f64, f64)> = Vec::with_capacity(cards);
    for _ in 0..trials {
        seen.clear();
        for _ in 0..cards {
            if !rng.chance(joker_share) {
                continue;
            }
            let roll = rng.unit();
            let r = if roll > 0.95 { 3 } else if roll > 0.7 { 2 } else { 1 };
            let list = &by_rarity[r];
            if list.is_empty() {
                continue;
            }
            let c = list[rng.below(list.len())];
            let gain = value(c) - now;
            if gain > 0.0 {
                seen.push((gain, c.cost as f64));
            }
        }
        seen.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (mut left, mut got, mut weight) = (budget, 0.0, 1.0);
        let mut bought = 0;
        for &(gain, cost) in &seen {
            if bought == open_slots {
                break;
            }
            if cost <= left {
                left -= cost;
                got += gain * weight;
                weight *= 0.6;
                bought += 1;
            }
        }
        total += now + got;
    }
    total / trials as f64
}

/// Play styles, grouped by what each joker's own effect rewards (from its definition in
/// the game data, not from strategy guides). Jokers that help every style (plain ×Mult,
/// +Mult) aren't in any group: this compares directions, not raw power.
pub fn archetypes() -> Vec<(&'static str, &'static [&'static str], &'static [crate::engine::HandType])> {
    use crate::engine::HandType::*;
    vec![
        ("Flushes", &["j_droll", "j_crafty", "j_tribe", "j_smeared", "j_four_fingers", "j_greedy_joker", "j_lusty_joker",
            "j_wrathful_joker", "j_gluttenous_joker", "j_bloodstone", "j_onyx_agate", "j_arrowhead", "j_ancient"], &[Flush, StraightFlush, FlushHouse, FlushFive]),
        ("Pairs & sets", &["j_jolly", "j_sly", "j_duo", "j_mad", "j_clever", "j_trousers", "j_zany", "j_wily", "j_trio", "j_family"],
            &[Pair, TwoPair, ThreeOfAKind, FullHouse, FourOfAKind, FiveOfAKind]),
        ("Straights", &["j_crazy", "j_devious", "j_order", "j_runner", "j_shortcut", "j_four_fingers"], &[Straight, StraightFlush]),
        ("Held cards", &["j_raised_fist", "j_baron", "j_mime", "j_shoot_the_moon", "j_steel_joker", "j_blackboard"], &[HighCard]),
        ("Face cards", &["j_scary_face", "j_smiley", "j_sock_and_buskin", "j_photograph", "j_hanging_chad", "j_pareidolia", "j_triboulet"], &[]),
        ("Small hands", &["j_half", "j_hanging_chad", "j_splash", "j_square"], &[]),
        ("Card ranks", &["j_fibonacci", "j_hack", "j_wee", "j_walkie_talkie", "j_even_steven", "j_odd_todd", "j_scholar"], &[]),
    ]
}

fn archetype_outlook(
    ctx: &Ctx,
    run: &RunState,
    data: &GameData,
    pool: &[Candidate],
    shares: &[f64],
    hand_mix: &[HandShare],
) -> Option<Outlook> {
    use crate::engine::HandType;
    // Target: a plain boss two antes ahead
    let ante = run.ante + 2;
    let target = crate::save::blind_amount(ante, run.blind_scaling) * 2.0 * run.ante_scaling;
    let spec = Spec {
        label: format!("Ante {ante}"),
        blind_key: String::new(),
        blind_name: "plain boss".into(),
        start: RoundStart {
            hand: vec![],
            deck: ctx.fresh_deck.clone(),
            hand_size: run.hand_size,
            hands: run.round_hands,
            discards: run.round_discards,
            scored: 0.0,
            target,
        },
        rules: RoundRules::default(),
        in_progress: false,
        horizon: true,
    };
    // Comparing styles on means, not win odds, so fewer simulations are enough.
    let sims = 32;
    let reach = |b: &Board| ctx.odds_one(b, &spec, sims).1.mean / target;
    let now_reach = reach(&ctx.base);

    // Which style do your points come from now?
    let points_in = |hands: &[HandType]| -> f64 {
        hand_mix.iter().filter(|h| HandType::from_name(&h.hand).is_some_and(|t| hands.contains(&t))).map(|h| h.share).sum()
    };
    let styles_def = archetypes();
    let current = styles_def
        .iter()
        .enumerate()
        .filter(|(_, (_, _, hands))| !hands.is_empty())
        .max_by(|a, b| points_in(a.1 .2).total_cmp(&points_in(b.1 .2)))
        .map(|(i, _)| i);

    let in_pool = |k: &str| pool.iter().find(|c| c.key == k);
    let styles: Vec<Style> = par_map(&styles_def.iter().enumerate().collect::<Vec<_>>(), |(i, (name, members, _))| {
        let owned: Vec<String> =
            ctx.base.jokers.iter().filter(|j| members.contains(&j.key.as_str())).map(|j| data.name(&j.key).to_string()).collect();
        let avail: Vec<&Candidate> = members.iter().filter_map(|k| in_pool(k)).collect();
        // Add a joker: into a free slot, else in place of your weakest non-eternal joker outside this style.
        let add_to = |b: &Board, key: &str, replaced: &mut Vec<String>| -> Option<Board> {
            let j = Joker::from_key(key, data)?;
            if (b.jokers.len() as i64) < b.joker_slots {
                return Some(with_joker(b, j, b.jokers.len()));
            }
            let weakest = b
                .jokers
                .iter()
                .enumerate()
                .filter(|(_, x)| !members.contains(&x.key.as_str()))
                .filter(|(_, x)| !run.jokers.iter().any(|sj| sj.key == x.key && sj.eternal))
                .min_by(|(ia, a), (ib, bb)| {
                    let sa = ctx.base.jokers.iter().position(|y| y.key == a.key).and_then(|p| shares.get(p)).copied().unwrap_or(0.0);
                    let sb = ctx.base.jokers.iter().position(|y| y.key == bb.key).and_then(|p| shares.get(p)).copied().unwrap_or(0.0);
                    sa.total_cmp(&sb).then(ia.cmp(ib))
                })
                .map(|(i, _)| i)?;
            replaced.push(data.name(&b.jokers[weakest].key).to_string());
            Some(with_joker(&without(b, weakest), j, weakest))
        };
        // Greedy: best single, then the best partner for the top few singles.
        let mut singles: Vec<(f64, &Candidate)> = avail
            .iter()
            .filter_map(|c| {
                let mut r = Vec::new();
                add_to(&ctx.base, &c.key, &mut r).map(|b| (reach(&b), *c))
            })
            .collect();
        singles.sort_by(|a, b| b.0.total_cmp(&a.0));
        let reach_one = singles.first().map_or(now_reach, |s| s.0);
        let mut best: Option<(f64, Vec<&Candidate>, Vec<String>)> =
            singles.first().map(|(r, c)| (*r, vec![*c], Vec::new()));
        for (_, first) in singles.iter().take(3) {
            for second in &avail {
                if second.key == first.key {
                    continue;
                }
                let mut replaced = Vec::new();
                let Some(b1) = add_to(&ctx.base, &first.key, &mut replaced) else { continue };
                let Some(b2) = add_to(&b1, &second.key, &mut replaced) else { continue };
                let r = reach(&b2);
                if best.as_ref().is_none_or(|(br, _, _)| r > *br) {
                    best = Some((r, vec![*first, *second], replaced));
                }
            }
        }
        let (reach_two, picks, replaces) = best.unwrap_or((now_reach, vec![], vec![]));
        Style {
            name: name.to_string(),
            current: current == Some(*i),
            points_now: points_in(styles_def[*i].2),
            owned,
            add: picks.iter().map(|c| (c.name.clone(), c.per_shop.unwrap_or(0.0), c.cost)).collect(),
            replaces,
            reach_one,
            reach: reach_two,
        }
    });
    let mut styles = styles;
    styles.sort_by(|a, b| b.reach.total_cmp(&a.reach));
    Some(Outlook { target_label: format!("Ante {ante} boss"), target, now_reach, styles })
}

fn desc_ctx(run: &RunState) -> crate::describe::DescCtx {
    let t = &run.round_targets;
    let full = run.full_deck().len() as i64;
    crate::describe::DescCtx {
        probability: run.probability_normal,
        dollars: run.dollars,
        starting_deck_size: run.starting_deck_size,
        playing_cards: full,
        deck_cards: run.draw_pile.len() as i64,
        jokers: run.jokers.len() as i64,
        tarots_used: run.tarots_used,
        skips: run.skips,
        idol_rank: t.idol.map_or(String::new(), |(r, _)| rank_name(r)),
        idol_suit: t.idol.map_or(String::new(), |(_, s)| s.name().to_string()),
        castle_suit: t.castle_suit.map_or(String::new(), |s| s.name().to_string()),
        ancient_suit: t.ancient_suit.map_or(String::new(), |s| s.name().to_string()),
        mail_rank: t.mail_rank.map_or(String::new(), rank_name),
    }
}

fn rank_name(r: crate::model::Rank) -> String {
    match r.0 {
        11 => "Jack".into(),
        12 => "Queen".into(),
        13 => "King".into(),
        14 => "Ace".into(),
        n => n.to_string(),
    }
}

fn describe(key: &str, ability: &serde_json::Value, ctx: &crate::describe::DescCtx) -> Option<String> {
    crate::describe::texts()?.describe(key, ability, ctx)
}

/// A joker's size one ante (3 rounds, ~12 hands) from now, under a simple stated
/// assumption; `None` for jokers that don't grow or fade. Labelled everywhere it shows.
fn grow_one_ante(j: &Joker, hand_mix: &[HandShare], dollars: f64, interest_line: f64) -> Option<(Joker, String, bool)> {
    grow_antes(j, hand_mix, dollars, interest_line, 1.0)
}

/// `grow_one_ante` over `antes` antes (growth and fading scale with it; labels describe one ante).
fn grow_antes(j: &Joker, hand_mix: &[HandShare], dollars: f64, interest_line: f64, antes: f64) -> Option<(Joker, String, bool)> {
    // Growth you pay for (skipping packs, rerolling): one per ante you'd do anyway, plus
    // one per $5 of money above the interest line (cash that isn't earning anything).
    let spare = (dollars - interest_line).max(0.0);
    let buys = (1.0 + (spare / 5.0).floor()).min(6.0);
    let share = |hands: &[&str]| hand_mix.iter().filter(|h| hands.contains(&h.hand.as_str())).map(|h| h.played).sum::<f64>();
    let hands_per_ante = 12.0;
    let mut g = j.clone();
    let (label, fades) = match j.key.as_str() {
        "j_red_card" => {
            g.mult += 3.0 * buys * antes;
            (format!("+{} Mult after 1 ante if you skip {buys:.0} booster pack{} (1 you'd open anyway, plus 1 per $5 above the ${interest_line:.0} interest line)", 3.0 * buys, if buys > 1.0 { "s" } else { "" }), false)
        }
        "j_green_joker" => {
            g.mult += 4.0 * antes;
            ("+4 Mult after 1 ante (+1 per hand, −1 per discard)".to_string(), false)
        }
        "j_ride_the_bus" => {
            g.mult += 3.0 * antes;
            ("~+3 Mult after 1 ante (resets whenever a face card scores)".to_string(), false)
        }
        "j_trousers" => {
            let n = (hands_per_ante * share(&["Two Pair", "Full House"])).round();
            g.mult += 2.0 * n * antes;
            (format!("+{} Mult after 1 ante (~{n:.0} two pairs/full houses in your hands)", 2.0 * n), false)
        }
        "j_runner" => {
            let n = (hands_per_ante * share(&["Straight", "Straight Flush"])).round();
            g.extra.chips += 15.0 * n * antes;
            (format!("+{} Chips after 1 ante (~{n:.0} straights in your hands)", 15.0 * n), false)
        }
        "j_square" => {
            g.extra.chips += 4.0 * 3.0 * antes;
            ("+12 Chips after 1 ante (3 hands of exactly 4 cards)".to_string(), false)
        }
        "j_flash" => {
            g.mult += 2.0 * buys * antes;
            (format!("+{} Mult after 1 ante if you reroll {buys:.0} time{} (1 anyway, plus 1 per $5 above the ${interest_line:.0} interest line)", 2.0 * buys, if buys > 1.0 { "s" } else { "" }), false)
        }
        "j_castle" => {
            g.extra.chips += 3.0 * 7.0 * antes;
            ("+21 Chips after 1 ante (~7 discarded cards of its suit)".to_string(), false)
        }
        "j_wee" => {
            g.extra.chips += 8.0 * 4.0 * antes;
            ("+32 Chips after 1 ante (~4 scored 2s)".to_string(), false)
        }
        "j_hologram" => {
            g.x_mult += 0.25 * antes;
            ("+×0.25 after 1 ante, if 1 card is added to your deck".to_string(), false)
        }
        "j_constellation" => {
            g.x_mult += 0.2 * antes;
            ("+×0.2 after 1 ante, if you use 2 planets".to_string(), false)
        }
        "j_madness" => {
            g.x_mult += 1.0 * antes;
            ("+×1 after 1 ante (2 small/big blinds), but destroys a joker each time".to_string(), false)
        }
        "j_campfire" => {
            g.x_mult += 0.5;
            ("+×0.5 if you sell 2 cards before the boss (resets after it)".to_string(), false)
        }
        "j_popcorn" => {
            g.mult = (g.mult - 12.0 * antes).max(0.0);
            (format!("fades: {} Mult after 1 ante (−4 per round)", g.mult), true)
        }
        "j_ice_cream" => {
            g.extra.chips = (g.extra.chips - 5.0 * hands_per_ante * antes).max(0.0);
            (format!("fades: {} Chips after 1 ante (−5 per hand)", g.extra.chips), true)
        }
        "j_ramen" => {
            g.x_mult = (g.x_mult - 0.3 * antes).max(1.0);
            (format!("fades: ×{:.1} after 1 ante (−×0.01 per discarded card)", g.x_mult), true)
        }
        "j_selzer" => {
            g.kind = Kind::Other;
            ("fades: used up after 10 hands (about 1 ante)".to_string(), true)
        }
        _ => return None,
    };
    Some((g, label, fades))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn j(key: &str) -> Joker {
        Joker::from_key(key, GameData::bundled()).unwrap()
    }

    #[test]
    fn fading_jokers_only_go_down() {
        for key in ["j_popcorn", "j_ice_cream", "j_ramen"] {
            let base = j(key);
            let size = |x: &Joker| x.mult + x.extra.chips + x.x_mult;
            let mut last = size(&base);
            for antes in [1.0, 2.0, 4.0, 8.0] {
                let (g, _, fades) = grow_antes(&base, &[], 10.0, 25.0, antes).unwrap();
                assert!(fades, "{key} should be marked as fading");
                assert!(size(&g) <= last, "{key} grew after {antes} antes");
                last = size(&g);
            }
        }
    }

    #[test]
    fn growing_jokers_only_go_up_and_pay_to_grow_follows_spare_money() {
        let base = j("j_red_card");
        let poor = grow_antes(&base, &[], 10.0, 25.0, 1.0).unwrap().0.mult;
        let rich = grow_antes(&base, &[], 60.0, 25.0, 1.0).unwrap().0.mult;
        let longer = grow_antes(&base, &[], 10.0, 25.0, 3.0).unwrap().0.mult;
        assert!(poor > base.mult && rich > poor && longer > poor);
    }

    /// A synthetic run in the shop: a standard deck, the given jokers (key, edition,
    /// perishable rounds left), full slots, and the given jokers for sale.
    fn shop_run(owned: &[(&str, Option<Edition>, Option<i64>)], for_sale: &[&str]) -> RunState {
        let data = GameData::bundled();
        let card = |key: &str, edition: Option<Edition>, perishable: Option<i64>| crate::save::JokerCard {
            key: key.into(),
            name: data.name(key).to_string(),
            edition,
            eternal: false,
            perishable,
            rental: false,
            debuff: perishable == Some(0),
            cost: data.center(key).map_or(4, |c| c.cost),
            sell_value: 2,
            ability: crate::engine::joker::ability_from_config(&data.center(key).unwrap().config),
            pending_tag_edition: None,
        };
        let mut r: RunState = serde_json::from_value(serde_json::json!({
            "seed": "T", "won": false, "game_version": "", "screen": "shop", "stake": 1, "deck": "Red Deck",
            "ante": 2, "win_ante": 8, "blind_scaling": 1, "ante_scaling": 1.0, "round": 4, "dollars": 30.0,
            "interest_amount": 1, "interest_cap": 25, "money_per_hand": 1.0, "base_reroll_cost": 5, "skips": 0, "hands_played": 0,
            "tarots_used": 0, "starting_deck_size": 52, "most_played_hand": "", "hands_left": 4, "discards_left": 3,
            "round_hands": 4, "round_discards": 3, "hand_size": 8, "joker_slots": owned.len(), "consumable_slots": 2, "probability_normal": 1.0,
            "jokers": [], "consumables": [], "hand": [], "draw_pile": [], "discard_pile": [], "hand_levels": {}, "blinds": [],
            "current_blind": null, "shop": null, "open_pack": [], "vouchers": [], "tags": [],
            "round_targets": {"ancient_suit": null, "castle_suit": null, "idol": null, "mail_rank": null},
            "used_jokers": [], "pool_flags": [], "banned_keys": [],
            "shop_rates": {"joker": 20.0, "tarot": 4.0, "planet": 4.0, "spectral": 0.0, "playing_card": 0.0, "slots": 2},
            "snapshot": {"path": "x", "live": false, "age_secs": null, "caveats": []}
        }))
        .unwrap();
        r.screen = crate::save::Screen::Shop;
        r.draw_pile = crate::bench::standard_deck();
        r.jokers = owned.iter().map(|(k, e, p)| card(k, *e, *p)).collect();
        let blind = |slot: &str, key: &str, target: f64| crate::save::BlindSlot {
            slot: slot.into(), key: key.into(), name: data.blinds.iter().find(|b| b.key == key).map_or(String::new(), |b| b.name.clone()),
            state: "Upcoming".into(), target, reward: 4, skip_tag: None,
        };
        r.blinds = vec![blind("Small", "bl_small", 800.0), blind("Big", "bl_big", 1200.0), blind("Boss", "bl_wall", 3200.0)];
        r.shop = Some(crate::save::Shop {
            jokers: for_sale.iter().map(|k| card(k, None, None)).collect(),
            other_cards: vec![],
            boosters: vec![],
            vouchers: vec![],
            reroll_cost: 5,
        });
        r
    }

    fn quick() -> Options {
        Options { sims: 60, screen_sims: 20, hand_samples: 80, seed: 7, rescue_top: 2 }
    }

    fn shop_action(owned: &[(&str, Option<Edition>, Option<i64>)], buy: &str) -> String {
        let a = analyze(&shop_run(owned, &[buy]), GameData::bundled(), None, &quick());
        a.shop.iter().find(|c| c.key == buy).unwrap().action.clone()
    }

    #[test]
    fn a_swap_puts_x_mult_after_plus_mult() {
        // Selling the weak Joker frees the left slot; Cavendish (×3) must still go last.
        let action = shop_action(&[("j_joker", None, None), ("j_joker", Some(Edition::Holo), None)], "j_cavendish");
        assert_eq!(action, "replace Joker, put it rightmost");
    }

    #[test]
    fn a_perished_joker_is_sold_first_and_mr_bones_is_kept() {
        // Mr. Bones and the perished Scary Face both score 0: the perished one goes.
        let action = shop_action(&[("j_mr_bones", None, None), ("j_scary_face", None, Some(0)), ("j_joker", Some(Edition::Holo), None)], "j_cavendish");
        assert!(action.starts_with("replace Scary Face"), "{action}");
        let action = shop_action(&[("j_mr_bones", None, None), ("j_joker", None, None)], "j_cavendish");
        assert!(action.starts_with("replace Joker"), "Mr. Bones sold over a scoring joker: {action}");
    }

    #[test]
    fn close_win_chances_rank_by_the_long_run() {
        let opt = |label: &str, p: f64, long: Option<f64>| ShopOption {
            reach: None, label: label.into(), kind: "tarot".into(), cost: 0, p_win: p, note: String::new(), money_after: 0.0,
            interest_now: 0, interest_after: 0, unaffordable: false, money_gain: 0.0, long_mult: long, key: None, desc: None,
        };
        let mut v = vec![opt("noise-best", 0.993, None), opt("keeper", 0.984, Some(1.09)), opt("clearly-better", 0.80, Some(2.0))];
        rank_options(&mut v, 0.5);
        assert_eq!(v.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["keeper", "noise-best", "clearly-better"]);
        let mut v = vec![opt("low", 0.60, Some(3.0)), opt("high", 0.90, None)];
        rank_options(&mut v, 0.5);
        assert_eq!(v[0].label, "high", "a real win-chance gap beats the long run");
    }

    #[test]
    fn a_random_joker_is_worth_at_least_a_typical_find() {
        let c = |r: u8, m: f64| Candidate {
            key: String::new(), name: String::new(), rarity: String::new(), cost: 4, edition: None, action: "add".into(),
            p_win: vec![], p_win_delta: vec![], reach: vec![], reach_delta: vec![], score_gain: 0.0, missing_gold: false,
            rarity_n: r, precise: false, per_shop: None, roles: vec![], note: None, desc: None, growth: None, long_mult: Some(m), sell_note: None,
        };
        let mut rng = crate::engine::Rng::new(1);
        let weak = long_draw(&[c(1, 0.5), c(2, 0.7), c(3, 0.9)], 2, 1.0, &mut rng);
        assert!((weak - 1.0).abs() < 1e-9, "bad draws are sold, not kept: {weak}");
        let one = long_draw(&[c(1, 1.5), c(2, 1.5), c(3, 1.5)], 1, 1.0, &mut rng);
        assert!((one - 1.5).abs() < 1e-9);
        let rarely = long_draw(&[c(1, 2.0), c(2, 2.0), c(3, 2.0)], 1, 0.2, &mut rng);
        assert!(rarely > 1.0 && rarely < 1.4, "a card that's seldom a joker: {rarely}");
    }

    #[test]
    fn one_off_money_counts_once() {
        // +$12 once must not be worth as much as +$12 every ante would: bounded, above ×1.00.
        let mut r = shop_run(&[("j_red_card", None, None), ("j_joker", None, None)], &[]);
        r.shop.as_mut().unwrap().other_cards.push(crate::save::ItemCard { key: "c_hermit".into(), name: "The Hermit".into(), set: "Tarot".into(), cost: 3, edition: None, card: None });
        let a = analyze(&r, GameData::bundled(), None, &quick());
        let h = a.options.iter().find(|o| o.label == "The Hermit").expect("Hermit offered");
        let m = h.long_mult.expect("money cards get a By Ante 8 value");
        assert!(m > 1.0 && m < 1.35, "Hermit By Ante 8 ×{m}");
    }

    #[test]
    fn interest_matches_the_game() {
        assert_eq!(interest(4.0, 1, 25), 0);
        assert_eq!(interest(13.0, 1, 25), 2);
        assert_eq!(interest(80.0, 1, 25), 5); // capped at $25 held
        assert_eq!(interest(80.0, 1, 50), 10); // Seed Money
    }
}
