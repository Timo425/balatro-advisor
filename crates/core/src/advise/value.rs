//! **Valuation**: what a board, a joker, money or a planet is worth by Ante 8, in one measure.
//!
//! Every board is projected to the end of the run (growers grown, faders faded, perishables
//! that run out gone, planets bought with the money it leaves you) and scored on whole
//! simulated rounds; values are ratios against your board as it is (`l0`). Money is the one
//! channel economy flows through, in two forms: money you hold every ante (rent lowers it)
//! sets how many pack skips, rerolls and planets you buy each ante; one-off money (a price, a
//! sell-back, Temperance) buys them once. Nothing is valued twice.

use std::sync::{Mutex, OnceLock};

use super::{best_of_subsets, glass_presence, grow_antes, income_per_ante, interest, money_value_with, par_map, round_mods, sample_hands, seal_planets_a_round, target_race_priced, UsePrice, TARGET_MAX, Candidate, Ctx, HandShare, Spec, TarotValue};
use crate::engine::HandType;
use crate::data::GameData;
use crate::engine::{Board, Joker, Kind, RunEvent};
use crate::model::Card;
use crate::save::{JokerCard, RunState};
use crate::sim::{RoundRules, RoundStart};

/// Deck changes are small, so they get more rounds than the rest (and the same seeds).
pub(super) const TAROT_ROUNDS: usize = 300;
/// A board without a chips joker will likely find one by Ante 8: the typical find in deck
/// projections is then a +60 Chips joker, so card chips (Bonus, Stone) aren't valued as if
/// that gap stayed open.
const CHIP_JOKERS: &[&str] = &["j_stuntman", "j_bull", "j_banner", "j_scary_face", "j_arrowhead", "j_castle", "j_runner", "j_square",
    "j_wee", "j_ice_cream", "j_blue_joker", "j_sly", "j_wily", "j_clever", "j_devious", "j_crafty", "j_odd_todd", "j_stone", "j_hiker"];

/// Rent ($3 a round) comes out of the money that would buy planets, rerolls and pack skips:
/// each rental counts as one ante of rent less money held.
pub(super) const RENT_PER_ANTE: f64 = 9.0;

/// The cheapest booster pack ($4: game.lua P_CENTERS, a normal pack): what money pays to open,
/// or skip, one.
pub(super) const PACK_PRICE: f64 = 4.0;
/// Consumables held at the end of a round, besides the planets Blue Seal cards make there: they
/// take slots those planets need (card.lua `Card:get_end_of_round_effect`: a Blue Seal makes its
/// planet only while the consumables held are fewer than the slots). Measured in the calibration
/// log on the rounds that had Blue Seal cards to lose a planet to it: the consumables in each
/// round's first shop, less planets new since the last shop (only Blue Seals make planets at
/// round end): 1 held in 1 of 4 rounds, none in the rest (one run, rounds 10–13, 2 slots; over
/// all 13 rounds it was 7, mostly a Cryptid held while there were no Blue Seals). Taken as the
/// same count however many slots you have.
pub(super) const HELD_AT_ROUND_END: f64 = 1.0 / 4.0;
/// Booster packs an ante: 2 in each shop (game.lua: `for i = 1, 2` in the shop's booster
/// area, not refreshed by a reroll), 3 shops an ante. The most packs that can be skipped.
pub(super) const PACKS_PER_ANTE: f64 = 6.0;
/// Rerolls money buys: at most this many an ante (on top of the one you'd do anyway)
const REROLLS_BOUGHT_PER_ANTE_MAX: usize = 5;

/// What the `i`-th reroll in a shop costs: the base cost, +$1 for each one before it in the
/// same shop (`G.GAME.current_round.reroll_cost_increase`, reset every shop).
pub(super) fn reroll_cost(base: i64, i: usize) -> i64 {
    base + i as i64
}

/// What a joker that changes a card each round does by Ante 8 (`LongRun::round_decks`)
pub(super) struct RoundChange {
    /// The decks round by round (yours first, then after each round; read by the tests)
    #[cfg_attr(not(test), allow(dead_code))]
    pub decks: Vec<Vec<Card>>,
    /// What they make your run worth, as a deck change
    pub value: f64,
    /// The share of rounds it changes a card in (and takes your first hand)
    pub share: f64,
    /// What a hand fewer every round leaves of the projected score (`hand_cost`)
    pub hand: f64,
}

pub(super) struct LongRun<'a> {
    pub ctx: &'a Ctx<'a>,
    pub run: &'a RunState,
    pub data: &'a GameData,
    pub hand_mix: &'a [HandShare],
    pub antes_left: f64,
    /// The interest line (your interest cap in $)
    pub line: f64,
    pub top_hand: Option<crate::engine::HandType>,
    /// What empty slots get filled with (0 ×Mult, 1 Chips, 2 +Mult), slot by slot
    fill_types: Vec<u8>,
    long_spec: Spec,
    mods_now: (i64, i64, i64),
    /// Your jokers weaker by Ante 8 than a typical find (assumed replaced by then)
    replaced: OnceLock<Vec<bool>>,
    /// Your board's projected score: the baseline every value is a ratio of
    pub l0: f64,
    /// Every slot taken by jokers that last
    pub full: bool,
    /// Baselines with a typical find in place of each joker you could sell
    pub sell_base: Vec<Option<f64>>,
    /// `planet(h)` per hand type, computed once
    planets: OnceLock<Vec<f64>>,
    /// Your deck as it is, projected (`deck_stats`), computed once
    deck_base: OnceLock<crate::sim::Stats>,
    /// The same on fewer rounds (quick screens), by round count
    deck_base_n: Mutex<std::collections::HashMap<usize, crate::sim::Stats>>,
    /// Whether the spare money goes to shop events rather than main-hand levels, by the jokers
    /// that grow from events (`spare_to_events`)
    spare_choice: Mutex<std::collections::HashMap<String, bool>>,
    /// Your deck's money while scoring, per round, on given rounds (`deck_rounds`)
    deck_money: Mutex<std::collections::HashMap<(usize, usize), f64>>,
    /// A joker's decks round by round, by its key (`round_decks`)
    round_decks: Mutex<std::collections::HashMap<String, std::sync::Arc<RoundChange>>>,
}

/// What an option or event adds to your run, applied to the projected board the same way
/// everywhere (`LongRun::value`).
#[derive(Debug, Clone, Default)]
pub(super) struct Gain {
    /// One-off money (a price is negative), spent once (`LongRun::once`)
    pub money: f64,
    /// Extra money held every ante (an economy voucher's interest)
    pub held: f64,
    /// Planets used: (hand, how many). Each levels its hand and grows Constellation ×0.1
    /// (card.lua: Constellation grows on every planet used).
    pub planets: Vec<(HandType, f64)>,
    /// Planets used on hands that don't matter here: they only grow Constellation
    pub other_planets: f64,
    /// Levels on every hand, without planets (Black Hole)
    pub all_levels: i64,
    /// Booster packs skipped (`RunEvent::SkipPack`), shop rerolls (`RunEvent::Reroll`), blinds
    /// skipped (`RunEvent::SkipBlind`, whole ones: the game counts them): what the board grows
    /// from them (`Board::after`)
    pub pack_skips: f64,
    pub rerolls: f64,
    pub blind_skips: f64,
    /// Consumable slots more by Ante 8 (a voucher's, Crystal Ball): the planets your Blue Seal
    /// cards make with them (`LongRun::slot_planets`)
    pub consumable_slots: i64,
}

impl Gain {
    pub fn money(m: f64) -> Gain {
        Gain { money: m, ..Default::default() }
    }

    /// `n` of `ev`, on top of `self`
    pub fn with_event(mut self, ev: RunEvent, n: f64) -> Gain {
        match ev {
            RunEvent::SkipPack => self.pack_skips += n,
            RunEvent::Reroll => self.rerolls += n,
            RunEvent::SkipBlind => self.blind_skips += n,
        }
        self
    }

    fn events(&self) -> [(RunEvent, f64); 3] {
        [(RunEvent::SkipPack, self.pack_skips), (RunEvent::Reroll, self.rerolls), (RunEvent::SkipBlind, self.blind_skips)]
    }
}

/// What money pays for one `ev`: a pack to skip what the cheapest pack costs (`PACK_PRICE`, as
/// in `Spending`), a reroll a shop's first one (`reroll_cost`); a blind skip isn't bought.
pub(super) fn event_price(run: &RunState, ev: RunEvent) -> Option<f64> {
    match ev {
        RunEvent::SkipPack => Some(PACK_PRICE),
        RunEvent::Reroll => Some(reroll_cost(run.base_reroll_cost, 0).max(1) as f64),
        RunEvent::SkipBlind => None,
    }
}

/// The event money buys on `b`, with its price: the one its jokers grow most from per dollar
/// (the first in `RunEvent::ALL` on a tie, never the order the jokers sit in); `None` when
/// none grows. Both money paths use it (`fill_long`): money held and one-off money. Mult per dollar compares +Mult growers only, the only ones money buys
/// growth for now (Red Card, Flash Card); a ×Mult one would need its value measured.
pub(super) fn bought_event(b: &Board, run: &RunState) -> Option<(RunEvent, f64)> {
    let per_dollar = |ev: RunEvent, price: f64| b.jokers.iter().map(|j| j.mult_from(ev)).sum::<f64>() / price;
    RunEvent::ALL
        .into_iter()
        .filter_map(|ev| event_price(run, ev).map(|p| (ev, p)))
        .filter(|&(ev, p)| per_dollar(ev, p) > 0.0)
        .fold(None, |best: Option<(RunEvent, f64)>, (ev, p)| match best {
            Some((b, bp)) if per_dollar(b, bp) >= per_dollar(ev, p) => best,
            _ => Some((ev, p)),
        })
}

/// How many `ev` a `budget` buys in an ante: pack skips at `PACK_PRICE` each (a pack skipped
/// gives up what it holds, so it costs a pack), at most `PACKS_PER_ANTE`; rerolls at the
/// game's price (+$1 for each before it in the same shop: `reroll_cost`), spread over the
/// ante's 3 shops, at most `REROLLS_BOUGHT_PER_ANTE_MAX`. The one place this is counted: the
/// projection (`fill_long`) and the growth label (`grow_one_ante`) both read it.
pub(super) fn events_per_ante(run: &RunState, ev: RunEvent, budget: f64) -> f64 {
    events_bought_for(run, ev, budget).0
}

/// `events_per_ante`, with what they cost
fn events_bought_for(run: &RunState, ev: RunEvent, budget: f64) -> (f64, f64) {
    let budget = budget.max(0.0);
    match ev {
        RunEvent::SkipPack => {
            let n = (budget / PACK_PRICE).floor().min(PACKS_PER_ANTE);
            (n, n * PACK_PRICE)
        }
        RunEvent::Reroll => {
            // the k-th reroll of the ante is the (k / 3)-th in its shop (the one done anyway first)
            let mut spent = 0.0;
            let mut k = 0;
            while k < REROLLS_BOUGHT_PER_ANTE_MAX {
                let price = reroll_cost(run.base_reroll_cost, (k + 1) / 3).max(1) as f64;
                if spent + price > budget {
                    break;
                }
                spent += price;
                k += 1;
            }
            (k as f64, spent)
        }
        RunEvent::SkipBlind => (0.0, 0.0),
    }
}

/// The most of `ev` money buys in an ante: the `PACKS_PER_ANTE` packs the shops hold, or
/// `REROLLS_BOUGHT_PER_ANTE_MAX` rerolls (on top of the one done anyway)
fn event_cap(ev: RunEvent) -> f64 {
    match ev {
        RunEvent::SkipPack => PACKS_PER_ANTE,
        RunEvent::Reroll => REROLLS_BOUGHT_PER_ANTE_MAX as f64,
        RunEvent::SkipBlind => 0.0,
    }
}

/// `l` with `n` levels more (or fewer), added to the chips and Mult already there: a fractional
/// level counts as its share of a level, and one already in `l` (the projection's) is kept
/// (`Level::with_level` recomputes from the base values and would drop it); never below level 1.
pub(super) fn add_levels(l: crate::engine::Level, n: f64) -> crate::engine::Level {
    let floor = l.with_level(1);
    let mut out = l;
    out.chips = (l.chips + l.l_chips * n).max(floor.chips);
    out.mult = (l.mult + l.l_mult * n).max(floor.mult);
    out.level = (l.level as f64 + n).floor().max(1.0) as i64;
    out
}

/// A projected board (`LongRun::project_rent`) and the money held it was projected with,
/// which buys its shop events once its jokers are all in (`LongRun::fill_long`).
#[derive(Clone)]
pub(super) struct Projected {
    board: Board,
    dollars: f64,
    /// Money per ante from money jokers minus rent (in `dollars`), and your main hand's level
    /// before the projection's levels: `fill_long` decides what the spare money buys
    flow: f64,
    top_base: Option<crate::engine::Level>,
}

impl std::ops::Deref for Projected {
    type Target = Board;
    fn deref(&self) -> &Board {
        &self.board
    }
}

impl std::ops::DerefMut for Projected {
    fn deref_mut(&mut self) -> &mut Board {
        &mut self.board
    }
}

impl<'a> LongRun<'a> {
    pub fn new(ctx: &'a Ctx<'a>, hand_mix: &'a [HandShare], pool_entries: &[Candidate], per_rarity: [usize; 4]) -> LongRun<'a> {
        let (run, data) = (ctx.run, ctx.data);
        let antes_left = ((run.win_ante - run.ante) as f64 + 0.5).max(0.5);
        let top_hand = hand_mix.first().and_then(|h| crate::engine::HandType::from_name(&h.hand));
        // What the empty slots get filled with, in proportion to how often the shop offers
        // each type of joker: weighted by rarity odds (70/25/5 split over each rarity's pool),
        // each joker counted as ×Mult, Chips or +Mult from its game data. Common +Mult jokers
        // make a +15 Mult find the likeliest, so the scarce types are what an option adds.
        let fill_types: Vec<u8> = {
            let mut w = [0.0f64; 3]; // ×Mult, Chips, +Mult
            for c in pool_entries {
                let Some(j) = Joker::from_key(&c.key, data) else { continue };
                let r = c.rarity_n.min(3) as usize;
                let weight = [0.0, 0.7, 0.25, 0.05][r] / per_rarity[r].max(1) as f64;
                let t = if j.x_mult > 1.0 || j.extra.xmult > 0.0 {
                    0
                } else if j.t_chips > 0.0 || j.extra.chips > 0.0 || j.extra.chip_mod > 0.0 {
                    1
                } else if j.mult > 0.0 || j.t_mult > 0.0 || j.extra.s_mult > 0.0 || j.extra.mult > 0.0 {
                    2
                } else {
                    continue;
                };
                w[t] += weight;
            }
            if w.iter().sum::<f64>() <= 0.0 {
                w = [1.0, 1.0, 1.0];
            }
            // Slot by slot, the type furthest below its share so far
            let total: f64 = w.iter().sum();
            let mut got = [0.0f64; 3];
            (0..8)
                .map(|n| {
                    let t = (0..3).max_by(|&a, &b| (w[a] / total * (n + 1) as f64 - got[a]).total_cmp(&(w[b] / total * (n + 1) as f64 - got[b]))).unwrap_or(0);
                    got[t] += 1.0;
                    t as u8
                })
                .collect()
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
        // Hand size, hands and discards from the jokers on the projected board: the game's
        // numbers already include the jokers you own now, so only the difference counts.
        let mods_now: (i64, i64, i64) = ctx.base.jokers.iter().map(round_mods).fold((0, 0, 0), |a, m| (a.0 + m.0, a.1 + m.1, a.2 + m.2.max(-run.round_discards)));
        let mut lr = LongRun {
            ctx,
            run,
            data,
            hand_mix,
            antes_left,
            line: run.interest_cap as f64,
            top_hand,
            fill_types,
            long_spec,
            mods_now,
            replaced: OnceLock::new(),
            l0: 0.0,
            full: false,
            sell_base: vec![],
            planets: OnceLock::new(),
            deck_base: OnceLock::new(),
            deck_base_n: Mutex::new(Default::default()),
            deck_money: Mutex::new(Default::default()),
            round_decks: Mutex::new(Default::default()),
            spare_choice: Mutex::new(Default::default()),
        };
        {
            let before = lr.long_score(&lr.fill_long(lr.project(&|_| true, run.dollars), None, 0.0));
            let swap: Vec<bool> = par_map(&(0..run.jokers.len()).collect::<Vec<_>>(), |&i| {
                let sj = &run.jokers[i];
                if sj.eternal || !lr.lasts(sj) {
                    return false;
                }
                let mut b = lr.project(&|_| true, run.dollars);
                let pos = (0..i).filter(|&k| lr.lasts(&run.jokers[k])).count();
                if pos >= b.jokers.len() {
                    return false;
                }
                b.jokers[pos] = lr.stand_in(1.25, 0.0, 0.0);
                lr.long_score(&lr.fill_long(b, None, 0.0)) >= before
            });
            let _ = lr.replaced.set(swap);
        }
        lr.l0 = lr.long_score(&lr.fill_long(lr.project(&|_| true, run.dollars), None, 0.0));
        let lasting = run.jokers.iter().filter(|sj| lr.lasts(sj)).count() as i64;
        lr.full = lasting >= ctx.base.joker_slots;
        lr.sell_base = if lr.full {
            par_map(&(0..run.jokers.len()).collect::<Vec<_>>(), |&i| {
                (!run.jokers[i].eternal && lr.lasts(&run.jokers[i])).then(|| lr.long_score(&lr.fill_long(lr.project(&|k| k != i, run.dollars), None, 0.0)))
            })
        } else {
            vec![]
        };
        lr
    }

    pub fn lasts(&self, sj: &JokerCard) -> bool {
        sj.perishable.is_none_or(|r| r as f64 >= 3.0 * self.antes_left)
    }

    pub fn owned_rent(&self, keep: &dyn Fn(usize) -> bool) -> f64 {
        self.run.jokers.iter().enumerate().filter(|(i, sj)| keep(*i) && self.lasts(sj) && sj.rental).count() as f64 * RENT_PER_ANTE
    }

    /// Planets on the hand that earns most of your points: half a level per ante as a base,
    /// plus more when money sits above the interest line (runs vary from 0 to 10+ levels).
    /// `flow`: money per ante from money jokers minus rent (`dollars` already includes it).
    /// Above the interest line it's in the spare money; the part below the line counts too,
    /// at about $18 an ante for a planet level, so rent costs and income pays even when poor.
    pub fn levels_for(&self, dollars: f64, flow: f64) -> (f64, f64) {
        let per_ante = self.levels_per_ante(dollars, flow, (dollars - self.line).max(0.0));
        (per_ante, per_ante * self.antes_left)
    }

    /// `levels_for` with `to_levels` of the spare money (above the interest line) going to
    /// levels, the rest elsewhere (`fill_long`); money held at `dollars`, `flow` as there
    pub fn levels_per_ante(&self, dollars: f64, flow: f64, to_levels: f64) -> f64 {
        let line = self.line;
        let below = if flow < 0.0 { -(-flow).min((line - dollars).max(0.0)) } else { flow.min((line - (dollars - flow)).max(0.0)) };
        (0.5 + to_levels.max(0.0) / 20.0 + below / 18.0).clamp(0.0, 2.0)
    }

    /// The spare money levels can use before they top out (`levels_per_ante`'s cap of 2)
    fn levels_cap(&self, dollars: f64, flow: f64) -> f64 {
        ((2.0 - self.levels_per_ante(dollars, flow, 0.0)) * 20.0).max(0.0)
    }

    /// Money jokers you keep pay every ante, like rent in reverse; one the projection replaces
    /// (`replaced`) isn't on the board it pays for.
    pub fn owned_income(&self, keep: &dyn Fn(usize) -> bool) -> f64 {
        let run = self.run;
        let replaced = |i: usize| self.replaced.get().is_some_and(|r| r[i]);
        run.jokers.iter().enumerate().filter(|(i, sj)| keep(*i) && self.lasts(sj) && !sj.debuff && !replaced(*i)).map(|(_, sj)| income_per_ante(&sj.key, &sj.ability, run, self.antes_left)).sum::<f64>()
    }

    /// Stand-ins for the jokers you'd find over the run: empty slots get alternating ×1.5,
    /// +60 Chips and +15 Mult jokers, and an option is compared against a ×1.25 "typical
    /// find" in its slot. An assumption, labelled as one.
    pub fn stand_in(&self, x: f64, m: f64, chips: f64) -> Joker {
        let mut j = Joker::from_key("j_joker", self.data).expect("j_joker in data");
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
    }

    /// The projected board: the jokers kept (by index), grown in rounds (`grow_antes`); what
    /// the money you'd hold buys comes with it (`Projected`). `extra_rent`: more rent per ante
    /// (negative: more income), on top of your jokers'.
    pub fn project_rent(&self, keep: &dyn Fn(usize) -> bool, dollars: f64, extra_rent: f64) -> Projected {
        let (ctx, run) = (self.ctx, self.run);
        let rent = self.owned_rent(keep) + extra_rent;
        let flow = self.owned_income(keep) - rent;
        let dollars = dollars + flow;
        let mut b = ctx.base.clone();
        b.blind = Default::default();
        let top_base = self.top_hand.map(|top| b.levels[top as usize]);
        if let (Some(top), Some(l)) = (self.top_hand, top_base) {
            b.levels[top as usize] = self.top_level(l, dollars, flow, (dollars - self.line).max(0.0));
        }
        b.jokers = ctx
            .base
            .jokers
            .iter()
            .zip(&run.jokers)
            .enumerate()
            .filter(|(i, (_, sj))| keep(*i) && self.lasts(sj))
            .map(|(i, (j, _))| {
                if self.replaced.get().is_some_and(|r| r[i]) {
                    self.stand_in(1.25, 0.0, 0.0)
                } else {
                    grow_antes(j, self.hand_mix, self.antes_left).map_or_else(|| j.clone(), |g| g.0)
                }
            })
            .collect();
        Projected { board: b, dollars, flow, top_base }
    }

    /// Your main hand's level by Ante 8 with money held at `dollars`, `to_levels` of the spare
    /// money going to levels (`levels_per_ante`)
    fn top_level(&self, l: crate::engine::Level, dollars: f64, flow: f64, to_levels: f64) -> crate::engine::Level {
        add_levels(l, self.levels_per_ante(dollars, flow, to_levels) * self.antes_left)
    }

    pub fn project(&self, keep: &dyn Fn(usize) -> bool, dollars: f64) -> Projected {
        self.project_rent(keep, dollars, 0.0)
    }

    /// One-off money spent once: `once` grows Constellation through the planets it buys, and
    /// `levels` (what the events didn't take, `fill_long`) buys planets for your main hand.
    fn spend_once(&self, b: &mut Board, once: f64, levels: f64) {
        // A level of your main hand about $12 (its planet is only in some shops and packs: a
        // Celestial pack holds it ~1 time in 4); any planet about $4, and each one used grows
        // Constellation by ×0.1.
        for j in b.jokers.iter_mut().filter(|j| j.key == "j_constellation") {
            j.x_mult = (j.x_mult + 0.1 * once / 4.0).max(1.0);
        }
        if let Some(top) = self.top_hand {
            // (a fractional level counts: rounding made any small purchase cost a whole level)
            b.levels[top as usize] = add_levels(b.levels[top as usize], levels / 12.0);
        }
    }

    /// The board with the option (or a typical find) in its slot, what money buys, and empty
    /// slots filled with stand-ins. Shop events: a reroll an ante done anyway for each joker that
    /// grows from one; a pack skipped only when bought (it gives up what the pack holds). The
    /// spare money (held above the interest line, every ante) is one budget for main-hand
    /// levels and the event `bought_event` picks (`events_per_ante`, up to what the shops hold):
    /// levels first (up to what `levels_per_ante` can use) and the rest on events, or events
    /// first and the rest on levels, whichever the projection scores higher (`spare_to_events`);
    /// each dollar is spent once. One-off money takes one route on every board: a price takes
    /// back events, then levels; money once goes to events on the events-first split (up to what
    /// the shops hold), else to levels; Constellation follows the planets it buys or loses.
    pub fn fill_long(&self, p: Projected, option: Option<Joker>, once: f64) -> Board {
        self.fill_split(p, option, once, None)
    }

    /// `fill_long` with the split forced (`Some(true)`: events first) or chosen (`None`)
    fn fill_split(&self, p: Projected, option: Option<Joker>, once: f64, events_first: Option<bool>) -> Board {
        let grower_option = option.as_ref().filter(|j| RunEvent::ALL.iter().any(|&ev| j.mult_from(ev) != 0.0 || j.xmult_from(ev) != 0.0)).cloned();
        let Projected { board: mut b, dollars, flow, top_base } = p;
        b.jokers.push(option.unwrap_or_else(|| self.stand_in(1.25, 0.0, 0.0)));
        if event_price(self.run, RunEvent::Reroll).is_some() && b.changes_with(RunEvent::Reroll) {
            b.after(RunEvent::Reroll, self.antes_left);
        }
        match bought_event(&b, self.run) {
            None => self.spend_once(&mut b, once, once),
            Some((ev, price)) => {
                let spare = (dollars - self.line).max(0.0);
                let split = |to_levels: f64, to_events: f64, first: bool| {
                    let mut e = b.clone();
                    if let (Some(top), Some(l)) = (self.top_hand, top_base) {
                        e.levels[top as usize] = self.top_level(l, dollars, flow, to_levels);
                    }
                    let held = events_per_ante(self.run, ev, to_events) * self.antes_left;
                    let room = (event_cap(ev) * self.antes_left - held).max(0.0) * price;
                    let to_events_once = if once < 0.0 { once.max(-held * price) } else if first { once.min(room) } else { 0.0 };
                    e.after(ev, held + to_events_once / price);
                    let to_levels_once = once - to_events_once;
                    self.spend_once(&mut e, to_levels_once, to_levels_once);
                    e
                };
                let to_levels = spare.min(self.levels_cap(dollars, flow));
                let levels_first = || split(to_levels, spare - to_levels, false);
                let events_first_board = || {
                    let spent = events_bought_for(self.run, ev, spare).1;
                    split(spare - spent, spent, true)
                };
                let first = match events_first {
                    Some(c) => c,
                    None => self.spare_to_events(&b, grower_option, &mut || (self.fill_slots(levels_first()), self.fill_slots(events_first_board()))),
                };
                b = if first { events_first_board() } else { levels_first() };
            }
        }
        self.fill_slots(b)
    }

    /// Empty slots filled with stand-ins (the jokers you'd find over the run)
    fn fill_slots(&self, mut b: Board) -> Board {
        let mut k = 0;
        while (b.jokers.len() as i64) < b.joker_slots {
            b.jokers.push(match self.fill_types.get(k).copied().unwrap_or(0) {
                0 => self.stand_in(1.5, 0.0, 0.0),
                1 => self.stand_in(1.0, 0.0, 60.0),
                _ => self.stand_in(1.0, 15.0, 0.0),
            });
            k += 1;
        }
        b
    }

    /// Whether the spare money goes events first rather than levels first, whichever the
    /// projection scores higher (events only when clearly higher: within `compare::EQUAL` it's
    /// levels, as on a board nothing grows on). Decided once for each set of jokers that grow
    /// from events, on one board: your projected board with the option that grows (if it's one)
    /// in the typical find's slot, slots filled; until the projection knows which of your
    /// jokers it replaces, on `board`'s own two splits (`these`), not kept.
    fn spare_to_events(&self, board: &Board, option: Option<Joker>, these: &mut dyn FnMut() -> (Board, Board)) -> bool {
        let pick = |(levels, events): (Board, Board)| self.long_score(&events) > self.long_score(&levels) * (1.0 + super::compare::EQUAL);
        if self.replaced.get().is_none() {
            return pick(these());
        }
        let mut growers: Vec<String> = board
            .jokers
            .iter()
            .filter(|j| RunEvent::ALL.iter().any(|&ev| j.mult_from(ev) != 0.0 || j.xmult_from(ev) != 0.0))
            .map(|j| format!("{}:{}", j.key, RunEvent::ALL.iter().map(|&ev| j.mult_from(ev)).sum::<f64>()))
            .collect();
        growers.sort();
        let key = growers.join(",");
        let mut cache = self.spare_choice.lock().unwrap();
        if let Some(&c) = cache.get(&key) {
            return c;
        }
        let p = self.project(&|_| true, self.run.dollars);
        let c = pick((self.fill_split(p.clone(), option.clone(), 0.0, Some(false)), self.fill_split(p, option, 0.0, Some(true))));
        cache.insert(key, c);
        c
    }

    /// Hand size, hands and discards from the jokers on the projected board. Turtle Bean has
    /// shrunk away by then (−1 hand size a round).
    pub fn long_spec_for(&self, b: &Board) -> Spec {
        let run = self.run;
        let later = b.jokers.iter().map(|j| if j.key == "j_turtle_bean" { (0, 0, 0) } else { round_mods(j) }).fold((0, 0, 0), |a, m| (a.0 + m.0, a.1 + m.1, a.2 + m.2.max(-run.round_discards)));
        let mut sp = self.long_spec.clone();
        sp.start.hand_size = (sp.start.hand_size + later.0 - self.mods_now.0).max(1);
        sp.start.hands = (sp.start.hands + later.1 - self.mods_now.1).max(1);
        sp.start.discards = (sp.start.discards + later.2 - self.mods_now.2).max(0);
        sp
    }

    /// A projected board's mean round score
    pub fn long_score(&self, b: &Board) -> f64 {
        self.ctx.odds_one(b, &self.long_spec_for(b), 48).1.mean.max(1.0)
    }

    /// Money is worth less the more you have: what limits a rich run is what the shops
    /// offer, not cash, so buying now doesn't stop you buying later. The value of money m is
    /// taken as 30·(1 − e^(−m/30)) (an assumption: the next dollar is worth about half at
    /// $20, a fifth at $47).
    pub fn spendable(&self, m: f64) -> f64 {
        30.0 * (1.0 - (-m.max(0.0) / 30.0).exp())
    }

    /// One-off money `delta` (a price, a sell-back) as money to spend, with its interest:
    /// dropping below an interest step costs that interest every round until you've saved
    /// back up (counted as one ante: 3 rounds); rising above one earns it.
    pub fn once(&self, delta: f64) -> f64 {
        let run = self.run;
        let per_round = |m: f64| interest(m, run.interest_amount, run.interest_cap) as f64;
        self.spendable(run.dollars + delta) - self.spendable(run.dollars) + 3.0 * (per_round(run.dollars + delta) - per_round(run.dollars))
    }

    /// A sellable option you'd later replace is worth at least a typical find, less what it
    /// costs net: its price minus what selling it gives back (Card:set_cost: half the price
    /// paid, at least $1; a rental sells for $1; a free one from a tag still sells for $1).
    /// A money joker earns while you hold it (about an ante before it's replaced), as a
    /// rental pays rent.
    pub fn floor_for(&self, cost: i64, rental: bool, key: &str) -> f64 {
        let (run, data) = (self.run, self.data);
        let sell = if rental { 1 } else { (cost / 2).max(1) };
        let rent = if rental { RENT_PER_ANTE } else { 0.0 };
        let ability = data.center(key).map(|c| crate::engine::joker::ability_from_config(&c.config)).unwrap_or_default();
        let earns = income_per_ante(key, &ability, run, self.antes_left.min(1.0));
        self.long_score(&self.fill_long(self.project(&|_| true, run.dollars), None, self.once(earns - ((cost - sell) as f64) - rent))) / self.l0
    }

    /// What a joker that changes a card you pick each round (`consumable::round_card_effect`;
    /// DNA: a copy of the first hand's single card, which spends that hand) does to your deck
    /// by Ante 8. Each round, the cards its target search picks (`target_race`, the search a
    /// consumable's targets go through, its best set as for a consumable) in an opening hand
    /// drawn from the deck as it is by then (the copies so far in it, so a copied card is
    /// drawn more often), one sampled hand a round, each round's race on rounds of its own.
    /// What comes round by round is a flow, in the race and in the value alike (`UsePrice`):
    /// the hand a change spends (that round's share of a hand fewer every round, `hand_cost`
    /// on your projected board) and the planets its Blue Seal cards make (from the round
    /// it's made). The deck is valued as every projection is (`value.rs` header): the deck it
    /// ends with by Ante 8, as a grower's state by then (`deck_value`). Computed once per
    /// joker. Offered now (in the shop or an open pack), a race every round; otherwise (the dig
    /// list's pool) one an ante, its pick made for each of the ante's rounds: about a third of
    /// the cost, a coarser deck. `None`: not such a joker.
    pub fn round_decks(&self, key: &str) -> Option<std::sync::Arc<RoundChange>> {
        use crate::engine::consumable;
        let (effect, min, max) = consumable::round_card_effect(key)?;
        let mut cache = self.round_decks.lock().unwrap();
        if let Some(d) = cache.get(key) {
            return Some(d.clone());
        }
        let (ctx, run) = (self.ctx, self.run);
        let salt = self.data.center(key).map_or(0, |c| c.order as u64) << 16;
        let rounds = (3.0 * self.antes_left).floor() as usize;
        let hand = self.hand_cost(&self.fill_long(self.project(&|_| true, run.dollars), None, 0.0));
        let offered = run.shop.iter().flat_map(|s| &s.jokers).any(|j| j.key == key) || run.open_pack.iter().any(|c| c.key == key);
        let stride = if offered { 1 } else { 3 };
        let mut decks = vec![ctx.fresh_deck.clone()];
        let mut changed = 0usize;
        for r in (0..rounds).step_by(stride) {
            let d = decks.last().expect("your deck first").clone();
            let held = sample_hands(d.len(), run.hand_size, 1, ctx.opts.seed, salt + r as u64).remove(0);
            let pick = target_race_priced(self, &[(1.0, effect)], min, max, &d, &held, None, self.round_price(r, rounds, hand)).map(|t| t.sets[0].clone()).filter(|v| !v.is_empty());
            // the pick made in each round this race stands for (the targets' places don't move:
            // a change adds at the end)
            let mut cur = d;
            for _ in 0..stride.min(rounds - r) {
                let mut next = match &pick {
                    Some(set) => {
                        changed += 1;
                        consumable::apply(effect, &cur, set)
                    }
                    None => cur.clone(),
                };
                cur = next.clone();
                next.sort_by_key(Card::order_key);
                decks.push(next);
            }
        }
        // the rounds sampled stand for the 3 an ante left (a share of the last one included)
        let scale = 3.0 * self.antes_left / rounds.max(1) as f64;
        let value = if changed == 0 {
            1.0
        } else {
            let planets = decks[1..].iter().map(|d| self.seal_planets_round(d)).sum::<f64>() * scale;
            self.deck_value_with_planets(decks.last().expect("your deck first"), 0.0, planets)
        };
        let out = std::sync::Arc::new(RoundChange { decks, value, share: changed as f64 / rounds.max(1) as f64, hand });
        cache.insert(key.to_string(), out.clone());
        Some(out)
    }

    /// What a change made in round `r` of `rounds` costs and lasts (`round_decks`), each a flow:
    /// that round's share of a hand fewer every round (`hand`: `hand_cost`), the Blue Seal
    /// planets from that round on; raced on rounds of its own
    pub(super) fn round_price(&self, r: usize, rounds: usize, hand: f64) -> UsePrice {
        UsePrice {
            factor: 1.0 - (1.0 - hand) / (3.0 * self.antes_left),
            planets_share: (rounds - r) as f64 / rounds.max(1) as f64,
            from: TAROT_ROUNDS + r * TARGET_MAX,
            // a round's copy not made is gone: the leader is taken on a tie (D8)
            keeps: false,
        }
    }

    /// What one hand fewer a round leaves of board `b`'s projected score (a share): what a
    /// joker that spends your first hand costs (DNA)
    fn hand_cost(&self, b: &Board) -> f64 {
        let mut sp = self.long_spec_for(b);
        sp.start.hands = (sp.start.hands - 1).max(1);
        self.ctx.odds_one(b, &sp, 48).1.mean.max(1.0) / self.long_score(b)
    }

    /// A joker that changes a card each round (`round_decks`) on its projected board `b`: the
    /// board with your first hand spent on it (DNA: a single card) in the share of rounds it
    /// changes a card in (a flow, at the price its race used), times what its deck makes your
    /// run worth. Any other joker: `long_score`.
    pub fn round_effect_long(&self, b: Board, key: &str) -> f64 {
        let full = self.long_score(&b);
        let Some(change) = self.round_decks(key) else { return full };
        full * (1.0 - change.share * (1.0 - change.hand)) * change.value
    }

    /// A joker's projected score: bought for `cost` (selling `sell`, if any), rent paid if
    /// rental, grown with the money you'd hold.
    pub fn long_of(&self, j: &Joker, sell: Option<usize>, cost: i64, rental: bool) -> f64 {
        let (run, data) = (self.run, self.data);
        let back = sell.map_or(0, |i| run.jokers[i].sell_value) as f64;
        let ability = data.center(&j.key).map(|c| crate::engine::joker::ability_from_config(&c.config)).unwrap_or_default();
        let rent = if rental { RENT_PER_ANTE } else { 0.0 };
        let income = income_per_ante(&j.key, &ability, run, self.antes_left);
        let horizon = if j.key == "j_madness" { 1.0 } else { self.antes_left };
        let g = grow_antes(j, self.hand_mix, horizon).map_or_else(|| j.clone(), |g| g.0);
        let b = self.fill_long(self.project_rent(&|k| Some(k) != sell, run.dollars, rent - income), Some(g), self.once(back - cost as f64));
        self.round_effect_long(b, &j.key)
    }

    /// Planets used on a board: whole levels and the rest as a share of a level's chips and
    /// mult; Constellation ×0.1 for each (and for each of `other`).
    pub fn add_planets(b: &mut Board, planets: &[(HandType, f64)], other: f64) {
        let n_all: f64 = planets.iter().map(|p| p.1).sum::<f64>() + other;
        for j in b.jokers.iter_mut().filter(|j| j.key == "j_constellation") {
            j.x_mult += 0.1 * n_all;
        }
        for &(h, n) in planets {
            b.levels[h as usize] = add_levels(b.levels[h as usize], n);
        }
    }

    /// Your projected board with `g` added
    pub fn board_with(&self, g: &Gain) -> Board {
        let mut b = self.fill_long(self.project(&|_| true, self.run.dollars + g.held), None, self.once(g.money));
        if g.all_levels != 0 {
            for l in b.levels.iter_mut() {
                *l = add_levels(*l, g.all_levels as f64);
            }
        }
        // more slots: more planets from your Blue Seal cards, on the hand they go to (as a deck's
        // are: `deck_round`)
        let (mut planets, mut other) = (g.planets.clone(), g.other_planets);
        let seal = self.slot_planets(g.consumable_slots);
        if seal != 0.0 {
            match self.top_hand {
                Some(top) => planets.push((top, seal)),
                None => other += seal,
            }
        }
        Self::add_planets(&mut b, &planets, other);
        for (ev, n) in g.events() {
            if n != 0.0 {
                b.after(ev, n);
            }
        }
        b
    }

    /// What `g` makes your run worth by Ante 8, as a ratio of your board as it is
    pub fn value(&self, g: &Gain) -> f64 {
        self.long_score(&self.board_with(g)) / self.l0
    }

    /// One `ev` by Ante 8 (`value`); exactly ×1.00 when it changes nothing on your board
    pub fn event(&self, ev: RunEvent) -> f64 {
        if !self.ctx.base.changes_with(ev) {
            return 1.0;
        }
        self.value(&Gain::default().with_event(ev, 1.0))
    }

    /// What `ev` grows on your board, for a note ("Red Card +3 Mult"); empty when nothing does
    pub fn event_note(&self, ev: RunEvent) -> String {
        let name = |j: &Joker| self.data.name(&j.key).to_string();
        self.ctx
            .base
            .jokers
            .iter()
            .filter_map(|j| match (j.mult_from(ev), j.xmult_from(ev)) {
                (m, _) if m != 0.0 => Some(format!("{} +{m} Mult", name(j))),
                (_, x) if x != 0.0 => Some(format!("{} +×{x}", name(j))),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// One planet of hand `h` used (its level, and Constellation), by Ante 8
    pub fn planet(&self, h: HandType) -> f64 {
        self.planets.get_or_init(|| par_map(&HandType::ALL, |&h| self.value(&Gain { planets: vec![(h, 1.0)], ..Default::default() })))[h as usize]
    }

    /// A changed deck projected to Ante 8 (the board with a typical chips find if it lacks
    /// chips, one-off money spent, money per ante held): its round scores, on more rounds
    /// than a board (deck changes are small).
    pub fn deck_stats(&self, d: &[Card], dollars: f64, money_once: f64, income: f64) -> crate::sim::Stats {
        self.deck_stats_n(d, dollars, money_once, income, TAROT_ROUNDS)
    }

    /// `deck_stats` on `rounds` rounds (fewer for a quick screen)
    pub fn deck_stats_n(&self, d: &[Card], dollars: f64, money_once: f64, income: f64, rounds: usize) -> crate::sim::Stats {
        self.deck_stats_s(d, dollars, money_once, income, rounds, 0.0, 0)
    }

    /// `deck_stats_n` with `planets` more of your main hand's planets used by then, on rounds
    /// `first`.. (round i always draws the same cards)
    #[allow(clippy::too_many_arguments)]
    fn deck_stats_s(&self, d: &[Card], dollars: f64, money_once: f64, income: f64, rounds: usize, planets: f64, first: usize) -> crate::sim::Stats {
        let (bb, start) = self.deck_round(d, dollars, money_once, income, planets);
        crate::sim::round_odds(&bb, &start, rounds, self.ctx.opts.seed.wrapping_add(first as u64 * 7919)).1
    }

    /// The projected round a deck is valued on (see `deck_stats`)
    fn deck_round(&self, d: &[Card], dollars: f64, money_once: f64, income: f64, planets: f64) -> (Board, RoundStart) {
        let has_chips = self.ctx.base.jokers.iter().any(|j| CHIP_JOKERS.contains(&j.key.as_str()));
        let find = if has_chips { None } else { Some(self.stand_in(1.0, 0.0, 60.0)) };
        let mut b = self.fill_long(self.project_rent(&|_| true, dollars, -income), find, money_once);
        if planets != 0.0 {
            match self.top_hand {
                Some(top) => Self::add_planets(&mut b, &[(top, planets)], 0.0),
                None => Self::add_planets(&mut b, &[], planets),
            }
        }
        let tally = |e: crate::model::Enhancement| d.iter().filter(|c| c.enhancement == Some(e)).count() as i64;
        b.steel_tally = tally(crate::model::Enhancement::Steel);
        b.stone_tally = tally(crate::model::Enhancement::Stone);
        b.driver_tally = d.iter().filter(|c| c.enhancement.is_some()).count() as i64;
        b.playing_cards = d.len() as i64;
        let mut sp = self.long_spec_for(&b);
        sp.start.deck = d.to_vec();
        let bb = self.ctx.board_for(&b, &sp);
        let start = self.ctx.start_for(&sp, &bb);
        (bb, start)
    }

    /// What a changed deck makes your run worth by Ante 8, as a ratio of your deck as it is:
    /// its score, plus what its cards make: the money they earn while scoring (Lucky cards,
    /// Gold Seals, Gold cards) as money you get every round (about 3 rounds an ante), and the
    /// planets of Blue Seal cards it has more (or fewer) of than yours
    /// (each drawn with `seal_round_chance` a round and held for its planet, 3 rounds an ante,
    /// at most the consumable slots left free at round end: `seal_planets`).
    /// `money_once`: one-off money that comes with the change (already through `once`).
    /// `rounds`: projection rounds.
    pub fn deck_value(&self, d: &[Card], money_once: f64, rounds: usize) -> f64 {
        let base = if rounds == TAROT_ROUNDS {
            self.deck_base().clone()
        } else {
            let cached = self.deck_base_n.lock().unwrap().get(&rounds).copied();
            cached.unwrap_or_else(|| {
                let st = self.deck_stats_n(&self.ctx.fresh_deck, self.run.dollars, 0.0, 0.0, rounds);
                self.deck_base_n.lock().unwrap().insert(rounds, st);
                st
            })
        };
        self.value_against(d, money_once, rounds, 0, &base, None)
    }

    /// One of several random outcomes of a change (outcome `part`): valued on its own slice of
    /// rounds (`part`·`rounds` onwards), against your deck's full projection, so the outcomes
    /// together cover as many independent rounds as one full projection.
    pub fn deck_value_part(&self, d: &[Card], money_once: f64, rounds: usize, part: usize) -> f64 {
        let base = self.deck_base().clone();
        self.value_against(d, money_once, rounds, part * rounds, &base, None)
    }

    /// `deck_value` with the planets its Blue Seal cards make by Ante 8 given (a deck that
    /// changes over the run counts them round by round, not as its last deck's all run)
    pub fn deck_value_with_planets(&self, d: &[Card], money_once: f64, planets: f64) -> f64 {
        let base = *self.deck_base();
        self.value_against(d, money_once, TAROT_ROUNDS, 0, &base, Some(planets))
    }

    /// `planets`: what its Blue Seal cards make (`None`: `seal_planets`)
    fn value_against(&self, d: &[Card], money_once: f64, rounds: usize, first: usize, base: &crate::sim::Stats, planets: Option<f64>) -> f64 {
        // `income`: the money its cards earn as income (`None`: worked out from these rounds)
        let raw = |d: &[Card], income: Option<f64>| {
            let dollars = self.run.dollars;
            let planets = planets.unwrap_or_else(|| self.seal_planets(d));
            let mut st = self.deck_stats_s(d, dollars, money_once, income.unwrap_or(0.0), rounds, planets, first);
            let mut used = income.unwrap_or(0.0);
            if income.is_none() {
                let extra = (st.money - base.money) * 3.0;
                if extra.abs() > 0.5 {
                    st = self.deck_stats_s(d, dollars, money_once, extra, rounds, planets, first);
                    used = extra;
                }
            }
            (st.mean.max(1.0) / base.mean.max(1.0), used)
        };
        let (v, used) = raw(d, None);
        match without_new_glass(&self.ctx.fresh_deck, d) {
            Some(gone) => {
                let (p, _) = raw(&gone, Some(used));
                p + (v - p) * glass_presence(self.antes_left)
            }
            None => v,
        }
    }

    /// The planets a deck's extra Blue Seal cards make by Ante 8, against yours: a round's
    /// (`seal_planets_round`) every round, 3 rounds an ante
    pub(super) fn seal_planets(&self, d: &[Card]) -> f64 {
        self.seal_planets_round(d) * 3.0 * self.antes_left
    }

    /// The planets a deck's Blue Seal cards make in a round, against yours (`seal_planets_in`
    /// in a deck of its size, with your consumable slots by Ante 8)
    pub(super) fn seal_planets_round(&self, d: &[Card]) -> f64 {
        let slots = self.consumable_slots(0);
        // each deck at its own size: a deck change that removes cards draws your Blue Seals more
        self.seal_planets_in(d, slots) - self.seal_planets_in(&self.ctx.fresh_deck, slots)
    }

    /// Your consumable slots by Ante 8, `more` added (a projected quantity, as hands and hand
    /// size are: `long_spec_for`): today's, less the one each Negative consumable you hold adds
    /// (it goes when the card does: card.lua `Card:remove`, `queue_negative_removal`)
    pub(super) fn consumable_slots(&self, more: i64) -> i64 {
        let negative = self.run.consumables.iter().filter(|c| c.edition == Some(crate::model::Edition::Negative)).count() as i64;
        (self.run.consumable_slots - negative + more).max(0)
    }

    /// The planets deck `d`'s Blue Seal cards make in a round with `slots` consumable slots, the
    /// consumables held at round end (`HELD_AT_ROUND_END`, a mix of the whole counts either side
    /// of it) taking theirs: `seal_planets_a_round` on what's left
    fn seal_planets_in(&self, d: &[Card], slots: i64) -> f64 {
        let blue = d.iter().filter(|c| c.seal == Some(crate::model::Seal::Blue)).count();
        let lo = HELD_AT_ROUND_END.floor();
        let at = |held: f64| seal_planets_a_round(self.run, blue, d.len(), (slots as f64 - held).max(0.0) as usize);
        let w = HELD_AT_ROUND_END - lo;
        (1.0 - w) * at(lo) + if w > 0.0 { w * at(lo + 1.0) } else { 0.0 }
    }

    /// The planets your Blue Seal cards make by Ante 8 with `more` consumable slots (3 rounds an
    /// ante; 0 when `more` is 0)
    pub(super) fn slot_planets(&self, more: i64) -> f64 {
        if more == 0 {
            return 0.0;
        }
        let d = &self.ctx.fresh_deck;
        (self.seal_planets_in(d, self.consumable_slots(more)) - self.seal_planets_in(d, self.consumable_slots(0))) * 3.0 * self.antes_left
    }

    /// `deck_value` round by round, on rounds `range` (round i draws the same cards for every
    /// deck), so changes can be compared on the same draws (`compare::race`). `income`: the
    /// money per ante its cards earn, as found on an earlier batch (`None`: work it out from
    /// these rounds, against your deck's money on the same rounds); returned with the values,
    /// to pass on the next batch. `planets_share`: the share of the run its Blue Seal planets
    /// come for (1: all of it; a change made later in the run, less).
    pub fn deck_rounds(&self, d: &[Card], money_once: f64, range: std::ops::Range<usize>, income: Option<f64>, planets_share: f64) -> (Vec<f64>, f64) {
        let (v, used) = self.deck_rounds_raw(d, money_once, range.clone(), income, planets_share);
        match without_new_glass(&self.ctx.fresh_deck, d) {
            Some(gone) => {
                let (p, _) = self.deck_rounds_raw(&gone, money_once, range, Some(used), planets_share);
                (p.iter().zip(&v).map(|(p, v)| p + (v - p) * glass_presence(self.antes_left)).collect(), used)
            }
            None => (v, used),
        }
    }

    fn deck_rounds_raw(&self, d: &[Card], money_once: f64, range: std::ops::Range<usize>, income: Option<f64>, planets_share: f64) -> (Vec<f64>, f64) {
        let base = self.deck_base().clone();
        let planets = self.seal_planets(d) * planets_share;
        let rounds = |income: f64| {
            let (bb, start) = self.deck_round(d, self.run.dollars, money_once, income, planets);
            crate::sim::round_results(&bb, &start, range.clone(), self.ctx.opts.seed)
        };
        let (r, used) = match income {
            Some(x) => (rounds(x), x),
            None => {
                let r = rounds(0.0);
                let key = (range.start, range.end);
                let cached = self.deck_money.lock().unwrap().get(&key).copied();
                let yours = cached.unwrap_or_else(|| {
                    let (bb, start) = self.deck_round(&self.ctx.fresh_deck, self.run.dollars, money_once, 0.0, 0.0);
                    let m = crate::sim::round_results(&bb, &start, range.clone(), self.ctx.opts.seed).iter().map(|x| x.money).sum::<f64>() / range.len().max(1) as f64;
                    self.deck_money.lock().unwrap().insert(key, m);
                    m
                });
                let extra = (r.iter().map(|x| x.money).sum::<f64>() / r.len().max(1) as f64 - yours) * 3.0;
                if extra.abs() > 0.5 { (rounds(extra), extra) } else { (r, 0.0) }
            }
        };
        (r.iter().map(|x| x.total.max(1.0) / base.mean.max(1.0)).collect(), used)
    }

    /// Your deck as it is, projected the same way: the baseline deck changes are compared to
    pub fn deck_base(&self) -> &crate::sim::Stats {
        self.deck_base.get_or_init(|| self.deck_stats(&self.ctx.fresh_deck, self.run.dollars, 0.0, 0.0))
    }

    /// The joker a shop action ("replace NAME[, put it rightmost]") sells
    pub fn sell_index(&self, action: &str) -> Option<usize> {
        action.strip_prefix("replace ").map(|n| n.trim_end_matches(", put it rightmost")).and_then(|name| self.ctx.base.jokers.iter().position(|x| self.data.name(&x.key) == name))
    }

    /// A projected score as a ratio of the baseline (with a typical find in place of `sell`)
    pub fn ratio(&self, v: f64, sell: Option<usize>) -> f64 {
        v / sell.and_then(|i| self.sell_base.get(i).copied().flatten()).unwrap_or(self.l0)
    }
}

/// Money a rich run can't turn into more planet levels (`levels_for` reaches its 2 an ante at
/// the interest line + $30) goes on rerolls and packs, whichever split is worth more by Ante 8:
/// rerolls by the best jokers they find (`money_value_with`, by their By Ante 8), packs ($4, 2
/// a shop) by a typical pack's best pick, each further pack worth ×0.8 of the one before (a
/// heuristic). The typical pack: Arcana, Celestial, Buffoon, Spectral and Standard by their
/// shop weights (game.lua P_CENTERS: 4, 4, 1.2, 0.6, 4; Standard counted as nothing).
pub(super) struct Spending<'a> {
    lr: &'a LongRun<'a>,
    pool: &'a [Candidate],
    pack_gain: f64,
    shops: usize,
    rerolls: Mutex<std::collections::HashMap<i64, f64>>,
}

impl<'a> Spending<'a> {
    pub fn new(lr: &'a LongRun<'a>, pool: &'a [Candidate], tarots: &[TarotValue], tarot_long: &[f64]) -> Spending<'a> {
        let gain = |vals: Vec<f64>, k: usize| if vals.is_empty() { 0.0 } else { (best_of_subsets(&vals, k).0 - 1.0).max(0.0) };
        let arcana = gain(tarot_long.iter().zip(tarots).filter(|(_, t)| !t.spectral).map(|(v, _)| v.max(1.0)).collect(), 3);
        let spectral = gain(tarot_long.iter().zip(tarots).filter(|(_, t)| t.spectral).map(|(v, _)| v.max(1.0)).collect(), 2);
        let celestial = gain(HandType::ALL.iter().map(|&h| lr.planet(h).max(1.0)).collect(), 3);
        let buffoon = gain(pool.iter().map(|c| c.long_mult.unwrap_or(1.0).max(1.0)).collect(), 2);
        let pack_gain = (4.0 * arcana + 4.0 * celestial + 1.2 * buffoon + 0.6 * spectral) / (4.0 + 4.0 + 4.0 + 1.2 + 0.6);
        Spending { lr, pool, pack_gain, shops: (3.0 * lr.antes_left).round() as usize, rerolls: Mutex::new(Default::default()) }
    }

    fn rerolls(&self, excess: f64) -> f64 {
        let k = (excess / 5.0).round() as i64;
        if let Some(v) = self.rerolls.lock().unwrap().get(&k) {
            return *v;
        }
        let v = money_value_with(self.lr.ctx, self.pool, &|c: &Candidate| c.long_mult.unwrap_or(1.0), 1.0, k as f64 * 5.0, false, self.shops, false);
        self.rerolls.lock().unwrap().insert(k, v);
        v
    }

    /// What holding `m` is worth by Ante 8 through rerolls and packs (a factor; compare two
    /// amounts by their ratio)
    pub fn factor(&self, m: f64) -> f64 {
        let excess = (m - (self.lr.line + 30.0)).max(0.0);
        let mut best = self.rerolls(excess);
        let mut packs = 0.0;
        for p in 1..=(2 * self.shops) {
            if PACK_PRICE * p as f64 > excess {
                break;
            }
            packs += self.pack_gain * 0.8f64.powi(p as i32 - 1);
            best = best.max((1.0 + packs) * self.rerolls(excess - PACK_PRICE * p as f64));
        }
        best
    }
}

/// A deck with more Glass cards than yours, after the extra ones break: Glass destroys the card
/// when it breaks (card.lua `shatter`), so they're gone (the ones that don't match a Glass card
/// of yours first). What they add counts for the share of the antes left they last
/// (`glass_presence`); the rest of the change in full. `None`: no more Glass than yours.
pub(super) fn without_new_glass(yours: &[Card], d: &[Card]) -> Option<Vec<Card>> {
    let glass = |c: &Card| c.enhancement == Some(crate::model::Enhancement::Glass);
    let new = d.iter().filter(|c| glass(c)).count().saturating_sub(yours.iter().filter(|c| glass(c)).count());
    if new == 0 {
        return None;
    }
    let mut pool: Vec<Card> = yours.iter().filter(|c| glass(c)).copied().collect();
    let mut unmatched: Vec<usize> = vec![];
    let mut matched: Vec<usize> = vec![];
    for (i, c) in d.iter().enumerate().filter(|(_, c)| glass(c)) {
        match pool.iter().position(|y| y.same_kind(c)) {
            Some(k) => {
                pool.remove(k);
                matched.push(i);
            }
            None => unmatched.push(i),
        }
    }
    let mut gone: Vec<usize> = unmatched.into_iter().chain(matched).take(new).collect();
    gone.sort_unstable_by(|a, b| b.cmp(a));
    let mut out = d.to_vec();
    for i in gone {
        out.remove(i);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_glass_is_gone_once_it_breaks() {
        let yours = Card::parse_list("AS:glass KH 7D").unwrap();
        // a Glass card added: once broken it's destroyed, not plain
        let added = Card::parse_list("AS:glass KH 7D 2C:glass").unwrap();
        assert_eq!(without_new_glass(&yours, &added).unwrap().iter().map(Card::label).collect::<Vec<_>>(), vec!["A♠ [glass]", "K♥", "7♦"]);
        // Justice turns a card Glass: that card goes
        let turned = Card::parse_list("AS:glass KH:glass 7D").unwrap();
        assert_eq!(without_new_glass(&yours, &turned).unwrap().len(), 2);
        // a Glass card of yours changed (Strength, a seal): no new Glass
        let changed = Card::parse_list("2S:glass KH 7D").unwrap();
        assert!(without_new_glass(&yours, &changed).is_none());
    }
}
