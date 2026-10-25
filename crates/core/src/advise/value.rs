//! **Valuation**: what a board, a joker, money or a planet is worth by Ante 8, in one measure.
//!
//! Every board is projected to the end of the run (growers grown, faders faded, perishables
//! that run out gone, planets bought with the money it leaves you) and scored on whole
//! simulated rounds; values are ratios against your board as it is (`l0`). Money is the one
//! channel economy flows through, in two forms: money you hold every ante (rent lowers it)
//! sets how many pack skips, rerolls and planets you buy each ante; one-off money (a price, a
//! sell-back, Temperance) buys them once. Nothing is valued twice.

use std::sync::{Mutex, OnceLock};

use super::{best_of_subsets, glass_presence, grow_antes, interest, money_value_with, par_map, round_mods, sample_hands, seal_planets_a_round, target_race_priced, UsePrice, TARGET_MAX, Candidate, Ctx, HandShare, Spec, TarotValue};
use crate::engine::HandType;
use crate::data::GameData;
use crate::engine::{Board, Joker, Kind, RunEvent};
use crate::model::Card;
use crate::save::RunState;
use crate::sim::{JokerGrowth, RoundStart};

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
/// Rounds a board's money is measured on, on each of this ante's three plain blinds
/// (`LongRun::money_board`, `advise::plain_rounds_of`): the same rounds for every board, so a joker's
/// money is the difference on the same deals (a joker that rolls, Lucky cards or Bloodstone, shifts
/// the round's later rolls: noise `MONEY_NOISE` stands for)
pub(super) const MONEY_ROUNDS: usize = 32;
/// Money an ante that a measured difference in money must pass to count (`LongRun::money_difference`): below it,
/// what moved is taken as the draws', not the joker's (the deck path's threshold too, `value_against`)
pub(super) const MONEY_NOISE: f64 = 0.5;

/// What a change makes a round, `with` against `without` (`LongRun::money_board`,
/// `advise::plain_round_cash`: each round's cash, `None` for a loss): the mean of their
/// difference on the rounds both win (a lost round pays nothing that's spent, and a change in
/// which rounds are won isn't money), counted when it's clear of the draws: past `MONEY_NOISE` an
/// ante and `MONEY_CLEAR` standard errors of that mean (on few rounds won the error is large; a
/// joker that pays without changing the play has none). The one measure of a difference in a
/// round's money (whatif's too).
pub(crate) fn money_difference(with: &[Option<f64>], without: &[Option<f64>]) -> f64 {
    let d: Vec<f64> = with.iter().zip(without).filter_map(|(a, b)| Some((*a)? - (*b)?)).collect();
    if d.is_empty() {
        return 0.0;
    }
    let n = d.len() as f64;
    let mean = d.iter().sum::<f64>() / n;
    let se = if d.len() > 1 { (d.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0) / n).sqrt() } else { f64::INFINITY };
    if (3.0 * mean).abs() > MONEY_NOISE && mean.abs() > MONEY_CLEAR * se { mean } else { 0.0 }
}

/// The mean money of the rounds won (`advise::plain_round_cash`; nothing when none is)
pub(crate) fn won_mean(rounds: &[Option<f64>]) -> f64 {
    let won: Vec<f64> = rounds.iter().flatten().copied().collect();
    if won.is_empty() { 0.0 } else { won.iter().sum::<f64>() / won.len() as f64 }
}

/// What joker `j` pays a round besides how the round is played, over `antes` antes, on board `b`
/// of `run` (`LongRun::payout_besides`; whatif's at 0 antes: this round's rate)
pub(crate) fn payout_besides(j: &Joker, b: &Board, run: &RunState, antes: f64) -> f64 {
    let mut later = j.clone();
    later.payout_after_bosses(((antes - 1.0) / 2.0 + 1.0 / 3.0).max(0.0));
    later.round_end_dollars(b) - j.round_end_dollars(b) + j.sell_value_a_round() + interest(run.dollars, j.interest_added(), run.interest_cap) as f64
}

/// Standard errors a measured difference in a round's money must clear to count
/// (`LongRun::money_difference`): it's the draws' otherwise
pub(super) const MONEY_CLEAR: f64 = 2.0;

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

/// What the projection does with one of your jokers by Ante 8 (`LongRun::build`): the choice
/// that leaves the run most, judged by the run's value
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Fate {
    Kept,
    /// A typical find takes its place: its own slot, or the right end (`end`), whichever the
    /// projection scores higher
    Replaced { end: bool },
    /// Sold, no find: its sale counted once, its slot left empty to Ante 8 (the game's limit
    /// unchanged: a find in it is `Replaced`), a Negative's gone with it (card.lua
    /// `Card:remove_from_deck`)
    Sold,
}

pub(super) struct LongRun<'a> {
    pub ctx: &'a Ctx<'a>,
    pub run: &'a RunState,
    pub data: &'a GameData,
    /// Your jokers' growth in rounds, to Ante 8, and a pool joker's (`Growths`)
    pub growths: &'a Growths,
    pub antes_left: f64,
    /// The interest line (your interest cap in $)
    pub line: f64,
    pub top_hand: Option<crate::engine::HandType>,
    /// What empty slots get filled with (0 ×Mult, 1 Chips, 2 +Mult), slot by slot
    fill_types: Vec<u8>,
    long_spec: Spec,
    mods_now: (i64, i64, i64),
    /// Your round by Ante 8: hand size, hands and discards on your projected board
    /// (`long_spec_for`), what your Blue Seal cards are seen in (`seal_planets_in`); set once
    /// your board is known (`new`), before then no board's own round adds planets
    round: OnceLock<(i64, i64, i64)>,
    /// What the projection does with each of your jokers by Ante 8 (`Fate`): kept, or let go
    /// where that leaves no less, replaced by a typical find in their place (their place or the
    /// right end) or sold outright, their money (income, rent) gone with them
    fates: OnceLock<Vec<Fate>>,
    /// Each of your sellable jokers' better way to let it go (`Fate::Replaced` or `Fate::Sold`,
    /// whichever leaves more; a Negative's only `Sold`): what selling it now leads to
    /// (`sold_now`); `None` for one that can't be sold or doesn't last
    let_go: Vec<Option<Fate>>,
    /// A typical find's price (`find_price`): what a joker the projection replaces nets is its
    /// sale less this
    find_price: f64,
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
    /// The rounds money is measured on (`advise::plain_rounds_of`: your board, this ante's plain
    /// blinds)
    money_rounds: (Board, Vec<RoundStart>),
    /// A board's money round by round on them (`money_board`), by the board
    money: Mutex<std::collections::HashMap<String, std::sync::Arc<Vec<Option<f64>>>>>,
}

/// A typical find's price: the jokers the shops could offer (`pool`; every joker the game has
/// when it isn't known), each by its rarity's odds (70/25/5, as the stand-ins' types) over that
/// rarity's count, at its price (data/game.json `cost`)
fn find_price(pool: &[String], data: &GameData) -> f64 {
    let all: Vec<String>;
    let keys = if pool.is_empty() {
        all = data.centers.iter().filter(|c| c.set == "Joker" && matches!(c.rarity, Some(1..=3))).map(|c| c.key.clone()).collect();
        &all
    } else {
        pool
    };
    let rarity = |k: &str| data.center(k).and_then(|c| c.rarity).unwrap_or(0).min(3) as usize;
    let mut count = [0usize; 4];
    for k in keys {
        count[rarity(k)] += 1;
    }
    let (mut sum, mut weight) = (0.0, 0.0);
    for k in keys {
        let r = rarity(k);
        let w = [0.0, 0.7, 0.25, 0.05][r] / count[r].max(1) as f64;
        sum += w * data.center(k).map_or(0, |c| c.cost) as f64;
        weight += w;
    }
    if weight > 0.0 { sum / weight } else { 0.0 }
}

/// Antes left to play to the run's end (the one you're in counted half)
pub(super) fn antes_left(run: &RunState) -> f64 {
    ((run.win_ante - run.ante) as f64 + 0.5).max(0.5)
}

/// Simulated runs of rounds a joker's growth in rounds is measured on (`Growths`)
pub(super) const GROWTH_CHAINS: usize = 16;

/// How the jokers a round changes (`engine::grows_in_rounds`) grow or fade over the run,
/// measured from the simulated player's rounds (`sim::grown_over`) instead of a table: rounds
/// dealt anew from your deck at this ante's three plain targets in turn (no boss's rule or
/// size), 3 an ante, the jokers carried from round to round.
#[derive(Clone)]
pub(super) struct Growths {
    /// The rounds read (`sim::grown_over` checkpoints): one ante (3) and to Ante 8
    pub checkpoints: Vec<usize>,
    /// Rounds to Ante 8 (3 an ante left, `antes_left`)
    pub rounds: usize,
    /// Your jokers' (by place on your board)
    pub owned: Vec<Option<JokerGrowth>>,
    /// A joker the shop could offer, on your board as it is, by key
    pub pool: std::collections::HashMap<String, JokerGrowth>,
}

impl Growths {
    pub fn none() -> Growths {
        Growths { checkpoints: vec![], rounds: 0, owned: vec![], pool: Default::default() }
    }

    /// Rounds to Ante 8 and the checkpoints read: one ante (or less, near the end) and the end
    pub fn plan(antes_left: f64) -> (usize, Vec<usize>) {
        let rounds = ((3.0 * antes_left).round() as usize).max(1);
        let mut cps = vec![3.min(rounds), rounds];
        cps.dedup();
        (rounds, cps)
    }

    /// `j` as `g` measured it after `rounds` rounds (a checkpoint read; else `None`): its
    /// numbers moved by what the runs added (`JokerGrowth::numbers`), `Some(None)` if used up
    /// (it leaves the board, its slot free). Only for the joker measured (`g.key`): `None` for
    /// anything else put in its place (a typical find).
    pub fn grown(&self, j: &Joker, g: &JokerGrowth, rounds: usize) -> Option<Option<Joker>> {
        if j.key != g.key {
            return None;
        }
        let k = self.checkpoints.iter().position(|&c| c == rounds)?;
        let mut out = j.clone();
        Some(g.numbers(k, j.lasting()).map(|n| {
            out.mult = n[0].max(0.0);
            out.x_mult = n[1].max(1.0);
            out.extra.chips = n[2].max(0.0);
            out
        }))
    }

    /// Whether the runs use up your joker `i` by Ante 8 (in at least half of them)
    pub fn used_up(&self, i: usize) -> bool {
        self.owned.get(i).and_then(|g| g.as_ref()).is_some_and(|g| g.gone.last().is_some_and(|&x| x >= 0.5))
    }

    /// Which of your joker `i`'s numbers (Mult, ×Mult, Chips, as `Joker::lasting` counts them)
    /// the measured rounds moved (`None`: not measured): the ones its own effect changes, so a
    /// step on another (Ride the Bus's ×Mult, which the game reads as a ×Mult joker's) measures
    /// a number no round leaves different
    pub fn moved(&self, i: usize) -> Option<[bool; 3]> {
        let g = self.owned.get(i)?.as_ref()?;
        Some(std::array::from_fn(|f| g.at.iter().any(|a| (a[f] - g.start[f]).abs() > 1e-9)))
    }

    /// Your joker `i` (as `j`: itself, or changed) by Ante 8, if measured
    pub fn owned_by_end(&self, i: usize, j: &Joker) -> Option<Option<Joker>> {
        let g = self.owned.get(i)?.as_ref()?;
        self.grown(j, g, self.rounds)
    }

    /// A pool joker after `rounds` rounds, if measured
    pub fn pool_after(&self, j: &Joker, rounds: usize) -> Option<Option<Joker>> {
        self.grown(j, self.pool.get(&j.key)?, rounds)
    }
}

/// The steps a joker's Mult, ×Mult and Chips are valued on (`LongRun::joker_worth`): about what
/// the jokers a round changes add in a hand or two (Green Joker and Ride the Bus +1 Mult a hand,
/// Spare Trousers +2; Vampire ×0.1 an enhanced card, Lucky Cat ×0.25; Square +4 Chips, Wee +8,
/// Runner +15)
pub(super) const GROWTH_STEPS: [f64; 3] = [2.0, 0.25, 10.0];

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
    /// cards make with them (`LongRun::seal_planets_for`)
    pub consumable_slots: i64,
    /// Hand size, hands and discards more every round by Ante 8 (a voucher's: Paint Brush,
    /// Grabber, Wasteful): part of the projected round (`LongRun::long_spec_for`), so it's
    /// played with them, and your Blue Seal cards are seen more (`LongRun::seal_planets_for`)
    pub hand_size: i64,
    pub hands: i64,
    pub discards: i64,
    /// One of your jokers (by its place) as this now: what its numbers left at a round's end are
    /// worth (`LongRun::joker_worth`), grown from there as the projection grows it
    pub joker: Option<(usize, Joker)>,
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

    /// What it adds to a round: (hand size, hands, discards), as `round_mods` counts them
    pub fn round(&self) -> (i64, i64, i64) {
        (self.hand_size, self.hands, self.discards)
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
    /// Money per ante from the jokers (`income`) minus rent (in `dollars`), and your main hand's level
    /// before the projection's levels: `fill_long` decides what the spare money buys
    pub(super) flow: f64,
    top_base: Option<crate::engine::Level>,
    /// One-off money from the jokers the projection lets go (sold, or replaced by a find: their
    /// sales less the finds' prices), in dollars (`sales`) and as money to spend (`once`), spent
    /// with the option's (`fill_long`)
    once: f64,
    sales: f64,
    /// Slots a joker sold outright leaves empty to Ante 8 (`Fate::Sold`, not a Negative: the
    /// game's limit stays, so Joker Stencil counts them): no stand-in fills them
    empty: i64,
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
    pub fn new(ctx: &'a Ctx<'a>, hand_mix: &'a [HandShare], growths: &'a Growths, pool: &[String], per_rarity: [usize; 4]) -> LongRun<'a> {
        Self::build(ctx, hand_mix, growths, pool, per_rarity, false)
    }

    /// `new` with your jokers whose growth rounds measure (`engine::grows_in_rounds`) kept
    /// however they project, never let go (`fates`): what they'd be worth if kept and played for, the
    /// round's goals the growth runs are played to again (`advise.rs` `played_for`)
    pub fn played_for(ctx: &'a Ctx<'a>, hand_mix: &'a [HandShare], growths: &'a Growths, pool: &[String], per_rarity: [usize; 4]) -> LongRun<'a> {
        Self::build(ctx, hand_mix, growths, pool, per_rarity, true)
    }

    fn build(ctx: &'a Ctx<'a>, hand_mix: &'a [HandShare], growths: &'a Growths, pool: &[String], per_rarity: [usize; 4], keep_growers: bool) -> LongRun<'a> {
        let (run, data) = (ctx.run, ctx.data);
        let antes_left = antes_left(run);
        let top_hand = hand_mix.first().and_then(|h| crate::engine::HandType::from_name(&h.hand));
        // What the empty slots get filled with, in proportion to how often the shop offers
        // each type of joker: weighted by rarity odds (70/25/5 split over each rarity's pool),
        // each joker counted as ×Mult, Chips or +Mult from its game data. Common +Mult jokers
        // make a +15 Mult find the likeliest, so the scarce types are what an option adds.
        let fill_types: Vec<u8> = {
            let mut w = [0.0f64; 3]; // ×Mult, Chips, +Mult
            for key in pool {
                let Some(j) = Joker::from_key(key, data) else { continue };
                let r = data.center(key).and_then(|c| c.rarity).unwrap_or(0).min(3) as usize;
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
                fresh: true,
                ..Default::default()
            },
            horizon: true,
        };
        // Hand size, hands and discards from the jokers on the projected board: the game's
        // numbers already include what the jokers you own now gave when added (`round_mods`),
        // so only the difference counts; Burglar's comes as each blind is set, from the board
        // played (`Board::at_blind_set`).
        let mods_now: (i64, i64, i64) = ctx.base.jokers.iter().map(round_mods).fold((0, 0, 0), |a, m| (a.0 + m.0, a.1 + m.1, a.2 + m.2));
        let mut lr = LongRun {
            ctx,
            run,
            data,
            growths,
            antes_left,
            line: run.interest_cap as f64,
            top_hand,
            fill_types,
            long_spec,
            mods_now,
            round: OnceLock::new(),
            fates: OnceLock::new(),
            let_go: vec![],
            find_price: find_price(pool, data),
            l0: 0.0,
            full: false,
            sell_base: vec![],
            planets: OnceLock::new(),
            deck_base: OnceLock::new(),
            deck_base_n: Mutex::new(Default::default()),
            deck_money: Mutex::new(Default::default()),
            round_decks: Mutex::new(Default::default()),
            spare_choice: Mutex::new(Default::default()),
            money_rounds: super::plain_rounds_of(ctx),
            money: Mutex::new(Default::default()),
        };
        {
            let before = lr.long_score(&lr.fill_long(lr.project(&|_| true, run.dollars), None, 0.0));
            // Each joker you could sell kept, or let go where that leaves no less, by the run's
            // value: replaced by a typical find (not a Negative: its slot goes with it) or sold
            // outright, whichever leaves more (`let_go_score`: the money recomputed on the board
            // without it, its income and rent gone with it)
            let choice: Vec<(Fate, Option<Fate>)> = par_map(&(0..run.jokers.len()).collect::<Vec<_>>(), |&i| {
                if run.jokers[i].eternal || !lr.lasts(i) || (keep_growers && crate::engine::grows_in_rounds(&ctx.base.jokers[i].key)) {
                    return (Fate::Kept, None);
                }
                // (sold either way: its sale counted in both, a find's price on top for a find)
                let sold = (lr.let_go_score(i, Fate::Sold, lr.let_go_money(i, Fate::Sold)), Fate::Sold);
                let replaced = (!lr.negative(i)).then(|| {
                    let money = lr.let_go_money(i, Fate::Replaced { end: false });
                    let here = (lr.let_go_score(i, Fate::Replaced { end: false }, money), Fate::Replaced { end: false });
                    if !lr.end_differs(i) {
                        return here;
                    }
                    let last = lr.let_go_score(i, Fate::Replaced { end: true }, money);
                    if last > here.0 { (last, Fate::Replaced { end: true }) } else { here }
                });
                let (score, way) = match replaced {
                    Some(r) if r.0 >= sold.0 => r,
                    _ => sold,
                };
                (if score >= before { way } else { Fate::Kept }, Some(way))
            });
            let _ = lr.fates.set(choice.iter().map(|c| c.0).collect());
            lr.let_go = choice.into_iter().map(|c| c.1).collect();
        }
        let yours = lr.fill_long(lr.project(&|_| true, run.dollars), None, 0.0);
        let (_, s) = yours.at_blind_set(&lr.long_spec_for(&yours, (0, 0, 0)).start);
        let _ = lr.round.set((s.hand_size, s.hands, s.discards));
        lr.l0 = lr.long_score(&yours);
        let lasting = (0..run.jokers.len()).filter(|&i| lr.lasts(i)).count() as i64;
        // (a Negative that doesn't last takes its slot with it: `project_as`)
        let gone_slots = (0..run.jokers.len()).filter(|&i| !lr.lasts(i) && lr.negative(i)).count() as i64;
        lr.full = lasting >= ctx.base.joker_slots - gone_slots;
        lr.sell_base = if lr.full {
            par_map(&(0..run.jokers.len()).collect::<Vec<_>>(), |&i| {
                (!run.jokers[i].eternal && ctx.base.jokers[i].edition != Some(crate::model::Edition::Negative) && lr.lasts(i)).then(|| lr.long_score(&lr.fill_long(lr.project(&|k| k != i, run.dollars), None, 0.0)))
            })
        } else {
            vec![]
        };
        lr
    }

    /// Whether your joker `i` is still on your board by Ante 8: not a perishable that runs out
    /// before then (3 rounds an ante), nor one the rounds measured use up (`Growths::used_up`)
    pub fn lasts(&self, i: usize) -> bool {
        self.run.jokers[i].perishable.is_none_or(|r| r as f64 >= 3.0 * self.antes_left) && !self.growths.used_up(i)
    }

    /// What the projection does with your joker `i` (`fates`; all kept until it's decided)
    fn fate(&self, i: usize) -> Fate {
        self.fates.get().map_or(Fate::Kept, |f| f[i])
    }

    fn negative(&self, i: usize) -> bool {
        self.ctx.base.jokers[i].edition == Some(crate::model::Edition::Negative)
    }

    /// What letting your joker `i` go as `way` nets: its sale, less a typical find's price when
    /// one replaces it (`find_price`)
    fn let_go_money(&self, i: usize, way: Fate) -> f64 {
        self.run.jokers[i].sell_value as f64 - if matches!(way, Fate::Replaced { .. }) { self.find_price } else { 0.0 }
    }

    /// Your joker `i`'s place among the jokers the projection keeps in theirs (`let_go_score`)
    fn place_of(&self, i: usize) -> usize {
        (0..i).filter(|&k| self.lasts(k) && matches!(self.fate(k), Fate::Kept | Fate::Replaced { end: false })).count()
    }

    /// Whether the right end is another place for a find in place of your joker `i` than its own
    fn end_differs(&self, i: usize) -> bool {
        let rest = (0..self.run.jokers.len()).filter(|&k| k != i && self.lasts(k) && self.fate(k) != Fate::Sold).count();
        self.place_of(i) < rest
    }

    /// The projected score with your joker `i` let go as `way` (`Sold`: no find, its slot left
    /// empty; `Replaced`: a typical find in its place or at the right end), the other jokers as the
    /// projection holds them, `money` one-off money with their sales (one `once` on the sum: the
    /// sale against leaving is told exactly)
    fn let_go_score(&self, i: usize, way: Fate, money: f64) -> f64 {
        let run = self.run;
        let find = self.stand_in(1.25, 0.0, 0.0);
        let mut p = match way {
            // the find's money comes (`project_as`)
            Fate::Replaced { .. } => self.project_as(&|k| k != i, run.dollars, 0.0, None, Some(&find), (0, 0, 0)),
            _ => self.project_as(&|k| k != i, run.dollars, 0.0, None, None, (0, 0, 0)),
        };
        p.once = self.once(p.sales + money);
        match way {
            Fate::Replaced { end } => {
                let at = if end { p.jokers.len() } else { self.place_of(i).min(p.jokers.len()) };
                p.jokers.insert(at, find);
            }
            // (a Negative's slot already gone with it: `project_as`)
            _ if !self.negative(i) => p.empty += 1,
            _ => {}
        }
        self.long_score(&self.fill_long(p, None, 0.0))
    }

    /// Your joker `i` sold now, as a ratio of your board's projected score (`l0`): let go its
    /// better way (`let_go`; one that doesn't last to Ante 8 is gone by then anyway, its slot as
    /// the projection leaves it), its sale counted once; where the projection lets it go anyway,
    /// also the round it would be held for until the next shop (`round_flow`). `None`: it can't
    /// be sold (eternal), or the projection doesn't weigh letting it go (a joker that grows in
    /// rounds kept as played for: `played_for`).
    pub fn sold_now(&self, i: usize) -> Option<f64> {
        if self.run.jokers[i].eternal {
            return None;
        }
        let flow = if self.gone_anyway(i) { self.round_flow(i) } else { 0.0 };
        let score = match self.let_go.get(i).copied().flatten() {
            // (the way and slot the projection chose: its value as `l0` holds it)
            Some(way) => self.let_go_score(i, way, self.let_go_money(i, way) + flow),
            None if !self.lasts(i) => {
                let mut p = self.project(&|k| k != i, self.run.dollars);
                p.once = self.once(p.sales + self.run.jokers[i].sell_value as f64 + flow);
                self.long_score(&self.fill_long(p, None, 0.0))
            }
            None => return None,
        };
        Some(score / self.l0)
    }

    /// Whether the projection has your joker `i` gone by Ante 8 anyway: run out, used up, or let go
    pub fn gone_anyway(&self, i: usize) -> bool {
        !self.lasts(i) || self.fate(i) != Fate::Kept
    }

    /// What holding your joker `i` through the round ahead costs in money: its rent (card.lua
    /// `Card:calculate_rental`, at the end of each round: $3), less the money it makes in that
    /// round on your board as it is (`money_board`, `payout_besides` over the round or so it's
    /// held, as `floor_for`)
    fn round_flow(&self, i: usize) -> f64 {
        let all = |_: usize| true;
        let money = money_difference(&self.money_board(&all, None, (0, 0, 0), true), &self.money_board(&|k| k != i, None, (0, 0, 0), true)) + self.payout_besides(&self.money_rounds.0.jokers[i], self.antes_left.min(1.0));
        if self.run.jokers[i].rental { RENT_PER_ANTE / 3.0 - money } else { -money }
    }

    /// Rent of your rentals that are on the projected board: not one that runs out, nor one the
    /// projection lets go (`fates`: its rent goes with it, as its income does)
    pub fn owned_rent(&self, keep: &dyn Fn(usize) -> bool) -> f64 {
        self.run.jokers.iter().enumerate().filter(|(i, sj)| keep(*i) && self.lasts(*i) && sj.rental && self.fate(*i) == Fate::Kept).count() as f64 * RENT_PER_ANTE
    }

    /// Planets on the hand that earns most of your points: half a level per ante as a base,
    /// plus more when money sits above the interest line (runs vary from 0 to 10+ levels).
    /// `flow`: money per ante from the jokers minus rent (`dollars` already includes it).
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

    /// The money your jokers pay every ante on the board projected with the jokers `keep`s
    /// (those that last, one the projection replaces by its typical find, not one it sells:
    /// `fates`), `added`
    /// on it and `more` a round (a `Gain`'s), like rent in reverse: your board's money against
    /// it without the jokers the engine pays money for (`engine::PAYS_MONEY`), and that board's
    /// against yours, each on the same rounds (`money_difference`: so a copy of a money joker,
    /// Blueprint's, is counted once, and goes with the joker it copies), 3 rounds an ante; plus
    /// what they pay besides a round's play (`payout_besides`).
    fn income(&self, keep: &dyn Fn(usize) -> bool, added: Option<&Joker>, more: (i64, i64, i64)) -> f64 {
        let jokers = &self.money_rounds.0.jokers;
        let all = |_: usize| true;
        let paid = |i: usize| crate::engine::PAYS_MONEY.contains(&jokers[i].key.as_str());
        let yours = self.money_board(&all, None, (0, 0, 0), false);
        // (a money joker replaced by its find keeps the find in both)
        let unpaid = |i: usize| !(self.on_projection(&all, i) && paid(i));
        let anchor = if (0..jokers.len()).any(|i| !unpaid(i)) { money_difference(&yours, &self.money_board(&unpaid, None, (0, 0, 0), false)) } else { 0.0 };
        let same = added.is_none() && more == (0, 0, 0) && self.money_mask(keep, false) == self.money_mask(&all, false);
        let change = if same { 0.0 } else { money_difference(&self.money_board(keep, added, more, false), &yours) };
        let besides: f64 = (0..jokers.len()).filter(|&i| self.on_projection(keep, i)).map(|i| self.payout_besides(&jokers[i], self.antes_left)).sum::<f64>() + added.map_or(0.0, |j| self.payout_besides(j, self.antes_left));
        3.0 * (anchor + change + besides)
    }

    /// Whether your joker `i` is on the projected board with the jokers `keep`s as itself: kept,
    /// lasting and not let go (replaced by a typical find or sold: `fates`)
    fn on_projection(&self, keep: &dyn Fn(usize) -> bool, i: usize) -> bool {
        keep(i) && self.lasts(i) && self.fate(i) == Fate::Kept
    }

    /// The money each round pays on your board as it is (`now`), or as the projection holds it
    /// with the jokers `keep`s (one that doesn't last or that it sells left out, one replaced by
    /// its typical find: `fates`), with `added` on it and `more` hand size, hands and discards a round (a
    /// `Gain`'s): the round's cash as the game pays it for how it's played
    /// (`sim::RoundResult::cash`: scoring, discards, the round's end, hands and discards left),
    /// `None` for a round lost, on `MONEY_ROUNDS` of each of this ante's three plain blinds, the
    /// same rounds for every board (`advise::plain_rounds_of`, `advise::plain_round_cash`).
    /// Measured once for each board.
    fn money_board(&self, keep: &dyn Fn(usize) -> bool, added: Option<&Joker>, more: (i64, i64, i64), now: bool) -> std::sync::Arc<Vec<Option<f64>>> {
        let (bb, starts) = &self.money_rounds;
        let mask = self.money_mask(keep, now);
        let place = |i: usize| mask.as_bytes()[i] as char;
        let key = format!("{mask} {more:?} {}", added.map_or(String::new(), |j| serde_json::to_string(j).unwrap_or_default()));
        if let Some(m) = self.money.lock().unwrap().get(&key) {
            return m.clone();
        }
        let mut b = bb.clone();
        let mut gone: Vec<&Joker> = vec![];
        b.jokers = bb
            .jokers
            .iter()
            .enumerate()
            .filter_map(|(i, j)| match place(i) {
                'k' => Some(j.clone()),
                c => {
                    gone.push(j);
                    (c == 'r').then(|| self.stand_in(1.25, 0.0, 0.0))
                }
            })
            .collect();
        b.jokers.extend(added.cloned());
        let added: Vec<&Joker> = added.into_iter().collect();
        let starts: Vec<RoundStart> = starts
            .iter()
            .map(|s| {
                let mut s = super::apply_mods(s, &added, &gone);
                s.hand_size = (s.hand_size + more.0).max(1);
                s.hands = (s.hands + more.1).max(1);
                s.discards = (s.discards + more.2).max(0);
                s
            })
            .collect();
        let m = std::sync::Arc::new(super::plain_round_cash(&b, &starts, MONEY_ROUNDS, self.ctx.opts.seed, (self.run.money_per_hand, self.run.money_per_discard)));
        self.money.lock().unwrap().insert(key, m.clone());
        m
    }

    /// Each of your jokers on the money board (`money_board`): '-' left out (not kept, or, for
    /// the projection, gone by Ante 8 or sold), 'r' replaced by its typical find (`fates`), 'k'
    /// itself
    fn money_mask(&self, keep: &dyn Fn(usize) -> bool, now: bool) -> String {
        let fate = |i: usize| if now { Fate::Kept } else { self.fate(i) };
        (0..self.money_rounds.0.jokers.len())
            .map(|i| if !keep(i) || (!now && !self.lasts(i)) || fate(i) == Fate::Sold { '-' } else if fate(i) == Fate::Kept { 'k' } else { 'r' })
            .collect()
    }

    /// What your joker `i` makes a round: the round's money on your board with it against
    /// without it (`money_difference`), and what it pays besides (`payout_besides`)
    #[cfg(test)]
    pub fn owned_money(&self, i: usize) -> f64 {
        let all = |_: usize| true;
        money_difference(&self.money_board(&all, None, (0, 0, 0), false), &self.money_board(&|k| k != i, None, (0, 0, 0), false)) + self.payout_besides(&self.money_rounds.0.jokers[i], self.antes_left)
    }

    /// What joker `j` bought makes a round over `antes` antes on your board projected with the
    /// jokers `keep`s: the round's money with it against without it (`money_difference`), and
    /// what it pays besides (`payout_besides`)
    pub fn added_money(&self, keep: &dyn Fn(usize) -> bool, j: &Joker, antes: f64) -> f64 {
        money_difference(&self.money_board(keep, Some(j), (0, 0, 0), false), &self.money_board(keep, None, (0, 0, 0), false)) + self.payout_besides(j, antes)
    }

    /// What joker `j` pays a round besides how the round is played, over `antes` antes (game
    /// facts in the engine): its round-end payout's rises at the bosses ahead, the mean over the
    /// rounds ahead of the raises paid by then (an ante's Small and Big Blind after the bosses
    /// before it, its boss after its own: (antes − 1)/2 + 1/3; the plain blinds measured have no
    /// boss) (`Joker::payout_after_bosses`: Rocket), the sell value a round adds to it
    /// (`Joker::sell_value_a_round`: Egg; counted as money, as it's sold for it) and the interest
    /// it adds at your money (`Joker::interest_added`: To the Moon)
    fn payout_besides(&self, j: &Joker, antes: f64) -> f64 {
        payout_besides(j, &self.ctx.base, self.run, antes)
    }

    /// What a round you play pays on your board besides its reward and interest: its money on
    /// the rounds won (`money_board` as it is) and the sell value it adds
    /// (`Joker::sell_value_a_round`)
    pub fn round_money_now(&self) -> f64 {
        won_mean(&self.money_board(&|_| true, None, (0, 0, 0), true)) + self.ctx.base.jokers.iter().map(Joker::sell_value_a_round).sum::<f64>()
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

    /// The projected board: the jokers kept (by index), grown in rounds (measured, `Growths`;
    /// else `grow_antes`); what
    /// the money you'd hold buys comes with it (`Projected`). `extra_rent`: more rent per ante
    /// (negative: more income), on top of your jokers'.
    pub fn project_rent(&self, keep: &dyn Fn(usize) -> bool, dollars: f64, extra_rent: f64) -> Projected {
        self.project_as(keep, dollars, extra_rent, None, None, (0, 0, 0))
    }

    /// `project_rent` with joker `i` as `j` now (`changed`), grown from there. The money the
    /// jokers pay (`income`) on the board with the jokers kept, `added` (a joker bought: it goes
    /// in its slot in `fill_long`) and `more` (a `Gain`'s round), measured on this ante's rounds.
    pub(super) fn project_as(&self, keep: &dyn Fn(usize) -> bool, dollars: f64, extra_rent: f64, changed: Option<(usize, &Joker)>, added: Option<&Joker>, more: (i64, i64, i64)) -> Projected {
        let (ctx, run) = (self.ctx, self.run);
        let mut b = ctx.base.clone();
        let sold = |i: usize| keep(i) && self.lasts(i) && self.fate(i) == Fate::Sold;
        b.jokers = ctx
            .base
            .jokers
            .iter()
            .zip(&run.jokers)
            .enumerate()
            .filter(|(i, _)| keep(*i) && self.lasts(*i) && !sold(*i) && self.fate(*i) != Fate::Replaced { end: true })
            .map(|(i, (j, _))| {
                let j = changed.filter(|c| c.0 == i).map_or(j, |c| c.1);
                if self.fate(i) == (Fate::Replaced { end: false }) {
                    self.stand_in(1.25, 0.0, 0.0)
                } else {
                    // (one the runs use up doesn't last: left out above, its slot an empty one)
                    self.growths.owned_by_end(i, j).flatten().or_else(|| grow_antes(j, self.antes_left).map(|g| g.0)).unwrap_or_else(|| j.clone())
                }
            })
            .collect();
        // a find the projection puts at the right end goes there (`Fate::Replaced`: the slot it
        // was chosen in)
        let at_end = (0..run.jokers.len()).filter(|&i| keep(i) && self.lasts(i) && self.fate(i) == Fate::Replaced { end: true }).count();
        b.jokers.extend((0..at_end).map(|_| self.stand_in(1.25, 0.0, 0.0)));
        // A joker left off the board takes its slot with it when it's Negative (card.lua
        // `Card:remove_from_deck`: the joker limit −1, sold or destroyed); another the projection
        // sells leaves its slot empty (`Fate::Sold`: no find)
        let n = run.jokers.len();
        let on = |i: usize| keep(i) && self.lasts(i) && !sold(i);
        b.joker_slots -= (0..n).filter(|&i| !on(i) && self.negative(i)).count() as i64;
        let empty = (0..n).filter(|&i| sold(i) && !self.negative(i)).count() as i64;
        // (each joker it lets go is sold, replaced by a find or not: its sale, less the find's
        // price, once)
        let let_go = |i: usize| keep(i) && self.lasts(i) && self.fate(i) != Fate::Kept;
        let sales: f64 = (0..n).filter(|&i| let_go(i)).map(|i| self.let_go_money(i, self.fate(i))).sum();
        let once = self.once(sales);
        let rent = self.owned_rent(keep) + extra_rent;
        let flow = self.income(keep, added, more) - rent;
        let dollars = dollars + flow;
        let top_base = self.top_hand.map(|top| b.levels[top as usize]);
        if let (Some(top), Some(l)) = (self.top_hand, top_base) {
            b.levels[top as usize] = self.top_level(l, dollars, flow, (dollars - self.line).max(0.0));
        }
        Projected { board: b, dollars, flow, top_base, once, sales, empty }
    }

    /// What one more Mult, ×Mult and Chip on joker `i` (the numbers `Joker::lasting` counts,
    /// stepped from where they are now; only those its rounds move, `Growths::moved`, the rest
    /// nothing) is worth by Ante 8 (`value`, less 1), each measured on a
    /// step of its own (`GROWTH_STEPS`) and never below nothing (more never scores less), and
    /// what losing it is worth: its place to a typical find, the benchmark it's kept against
    /// (`fates`; a Negative's slot goes with it: no find), so less than nothing for every joker
    /// the projection keeps. Nothing for a joker the projection drops (a perishable that runs
    /// out) or lets go (replaced or sold, not eternal).
    pub fn joker_worth(&self, i: usize) -> ([f64; 3], f64) {
        if !self.lasts(i) || self.fate(i) != Fate::Kept {
            return ([0.0; 3], 0.0);
        }
        let j = &self.ctx.base.jokers[i];
        let fields = self.growths.moved(i).unwrap_or([true; 3]);
        let per = par_map(&[0, 1, 2], |&f| {
            if !fields[f] {
                return 0.0;
            }
            let mut c = j.clone();
            match f {
                0 => c.mult += GROWTH_STEPS[0],
                1 => c.x_mult += GROWTH_STEPS[1],
                _ => c.extra.chips += GROWTH_STEPS[2],
            }
            ((self.value(&Gain { joker: Some((i, c)), ..Default::default() }) - 1.0) / GROWTH_STEPS[f]).max(0.0)
        });
        // (destroyed: no sale)
        // (a Negative's slot goes with it; another's takes a find, where the projection would put one)
        let way = match self.let_go.get(i).copied().flatten() {
            _ if self.negative(i) => Fate::Sold,
            Some(r @ Fate::Replaced { .. }) => r,
            _ => Fate::Replaced { end: false },
        };
        let gone = self.let_go_score(i, way, 0.0) / self.l0 - 1.0;
        ([per[0], per[1], per[2]], gone)
    }

    /// Whether the projection lets your joker `i` go by Ante 8: replaced by a typical find or
    /// sold (`fates`)
    pub fn is_let_go(&self, i: usize) -> bool {
        self.fate(i) != Fate::Kept
    }

    /// Whether the projection sells your joker `i` outright by Ante 8 (`Fate::Sold`)
    pub fn is_sold(&self, i: usize) -> bool {
        self.fate(i) == Fate::Sold
    }

    /// What the numbers your jokers a round changes (`engine::CHANGE_IN_ROUND`) are left with at a
    /// round's end are worth (`joker_worth`), for `sim::RoundGoals::jokers`, on `board` (the
    /// round's: a joker debuffed there changes nothing); a joker worth nothing either way (one the
    /// projection drops or replaces) isn't followed. `floor`: whether a fall costs at most losing
    /// the joker (`JokerWorth::floor`; without it, only what its numbers are worth counts, so one
    /// worth only its loss, Seltzer, isn't followed). With `ahead` (Best play's round), each also
    /// counts what its numbers and losing it do in the blinds still ahead in this ante
    /// (`Ahead::joker`, kept apart from its Ante 8 worth: `sim::JokerWorth::of`): one the
    /// projection drops or replaces by Ante 8 too, as it's on your board for those blinds; and
    /// each joker's counter a round moves (`Joker::counter`, `Ahead::counter`: Loyalty Card's,
    /// Yorick's, Seltzer's), which counts there only
    pub fn round_worths(&self, floor: bool, ahead: Option<&Ahead>, board: &Board) -> Vec<crate::sim::JokerWorth> {
        let jokers = &self.ctx.base.jokers;
        // and those with a counter a round moves (`Joker::counter`), in the blinds ahead only
        let numbers = |i: usize| crate::engine::CHANGE_IN_ROUND.contains(&jokers[i].key.as_str());
        let counter = |i: usize| ahead.is_some() && jokers[i].counter(self.ctx.base.hands_played).is_some();
        let changed: Vec<usize> = (0..jokers.len()).filter(|&i| !board.jokers.get(i).is_some_and(|j| j.debuff) && (numbers(i) || counter(i))).collect();
        let worth = par_map(&changed, |&i| {
            let (per, gone) = if numbers(i) { self.joker_worth(i) } else { ([0.0; 3], 0.0) };
            let ahead = ahead.map(|a| {
                let w = if numbers(i) { a.joker(i, self.growths.moved(i).unwrap_or([true; 3])) } else { Default::default() };
                crate::sim::AheadWorth { counter: a.counter(i), ..w }
            });
            (per, gone, ahead.unwrap_or_default())
        });
        changed
            .iter()
            .zip(worth)
            .filter(|(_, (per, gone, a))| per.iter().any(|&x| x > 0.0) || a.counts() || (floor && *gone != 0.0))
            .map(|(&i, (per, gone, ahead))| crate::sim::JokerWorth { id: jokers[i].id, key: jokers[i].key.clone(), start: jokers[i].lasting(), per, gone, floor, ahead })
            .collect()
    }

    /// The round's goals as this projection values them (`sim::RoundGoals`), for Best play's
    /// rounds and the growth runs played to them (`advise.rs` `played_for`): a Blue Seal's planet
    /// by hand when `planets` (the hand played last: `planet`), a dollar won at `dollar`, a hand
    /// left at the run's money per hand, and the jokers a round changes (`round_worths`, `floor`,
    /// on `board`). With `ahead` (Best play's round), each also counts what it does in the blinds
    /// still ahead in this ante (`Ahead`: a planet's level, a dollar, the jokers' numbers, as
    /// shifts from the chance there, `RoundGoals::ahead`). Cards worth drawing (`seen`) are Best
    /// play's own.
    pub fn round_goals(&self, planets: bool, dollar: f64, floor: bool, ahead: Option<&Ahead>, board: &Board) -> crate::sim::RoundGoals {
        let (planet, planet_ahead): (Vec<f64>, Vec<f64>) = if planets {
            let ahead_levels = ahead.map_or(vec![0.0; 12], |a| par_map(&HandType::ALL, |&h| a.level(h)));
            (HandType::ALL.iter().map(|&h| (self.planet(h) - 1.0).max(0.0)).collect(), ahead_levels)
        } else {
            (vec![0.0; 12], vec![0.0; 12])
        };
        let dollar_ahead = ahead.map_or(0.0, |a| a.dollar());
        let jokers = self.round_worths(floor, ahead, board);
        // the chance the shifts are taken from, once one counts
        let counted = dollar_ahead > 0.0 || planet_ahead.iter().any(|&x| x > 0.0) || jokers.iter().any(|w| w.ahead.counts());
        let base = ahead.filter(|_| counted).map_or(0.0, |a| a.base());
        crate::sim::RoundGoals {
            planet: std::array::from_fn(|i| planet[i]),
            dollar,
            planet_ahead: std::array::from_fn(|i| planet_ahead[i]),
            dollar_ahead,
            ahead: base,
            per_hand: self.run.money_per_hand,
            jokers: std::sync::Arc::new(jokers),
            ..Default::default()
        }
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
        let Projected { board: mut b, dollars, flow, top_base, once: sold, empty, .. } = p;
        let once = once + sold;
        // (a Negative option brings its slot: card.lua `Card:add_to_deck`, the joker limit +1)
        if option.as_ref().is_some_and(|j| j.edition == Some(crate::model::Edition::Negative)) {
            b.joker_slots += 1;
        }
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
                    None => self.spare_to_events(&b, grower_option, &mut || (self.fill_slots(levels_first(), empty), self.fill_slots(events_first_board(), empty))),
                };
                b = if first { events_first_board() } else { levels_first() };
            }
        }
        self.fill_slots(b, empty)
    }

    /// Empty slots filled with stand-ins (the jokers you'd find over the run), but `empty` of
    /// them left empty (`Projected::empty`)
    fn fill_slots(&self, mut b: Board, empty: i64) -> Board {
        let mut k = 0;
        while (b.jokers.len() as i64) < b.joker_slots - empty {
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
    /// jokers it lets go, on `board`'s own two splits (`these`), not kept.
    fn spare_to_events(&self, board: &Board, option: Option<Joker>, these: &mut dyn FnMut() -> (Board, Board)) -> bool {
        let pick = |(levels, events): (Board, Board)| self.long_score(&events) > self.long_score(&levels) * (1.0 + super::compare::EQUAL);
        if self.fates.get().is_none() {
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

    /// Hand size, hands and discards from the jokers on the projected board, `more` (hand
    /// size, hands, discards: a `Gain`'s) added, before the blind is set (`Board::at_blind_set`
    /// on `b`). Turtle Bean has shrunk away by then (−1 hand size a round).
    pub fn long_spec_for(&self, b: &Board, more: (i64, i64, i64)) -> Spec {
        let later = b.jokers.iter().map(|j| if j.key == "j_turtle_bean" { (0, 0, 0) } else { round_mods(j) }).fold((0, 0, 0), |a, m| (a.0 + m.0, a.1 + m.1, a.2 + m.2));
        let mut sp = self.long_spec.clone();
        sp.start.hand_size = (sp.start.hand_size + later.0 - self.mods_now.0 + more.0).max(1);
        sp.start.hands = (sp.start.hands + later.1 - self.mods_now.1 + more.1).max(1);
        sp.start.discards = (sp.start.discards + later.2 - self.mods_now.2 + more.2).max(0);
        sp
    }

    /// A projected board's mean round score, on its own round (`long_spec_for`: the hand size,
    /// hands and discards its jokers give), with the planets your Blue Seal cards make more (or
    /// fewer) in that round than in yours (`seal_planets_for`)
    pub fn long_score(&self, b: &Board) -> f64 {
        self.long_score_more(b, (0, 0, 0), 0)
    }

    /// `long_score` with `more` hand size, hands and discards a round and `slots` consumable
    /// slots more (a `Gain`'s)
    fn long_score_more(&self, b: &Board, more: (i64, i64, i64), slots: i64) -> f64 {
        let sp = self.long_spec_for(b, more);
        let seal = self.seal_planets_in_round(slots, &sp, b);
        let score = |b: &Board| self.ctx.odds_one(b, &sp, 48).1.mean.max(1.0);
        if seal == 0.0 {
            return score(b);
        }
        // on the hand they go to (as a deck's are: `deck_round`)
        let mut b = b.clone();
        match self.top_hand {
            Some(top) => Self::add_planets(&mut b, &[(top, seal)], 0.0),
            None => Self::add_planets(&mut b, &[], seal),
        }
        score(&b)
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
    /// A money joker earns while you hold it (about an ante before it's replaced: its money a
    /// round on your board, `added_money`), as a rental pays rent. Scored on the board you'd hold: with
    /// `sold` (the joker the option replaces) gone and its sale counted, against the same
    /// baseline as the option (`ratio`).
    pub fn floor_for(&self, cost: i64, rental: bool, key: &str, sold: Option<usize>) -> f64 {
        let (run, data) = (self.run, self.data);
        let sell = if rental { 1 } else { (cost / 2).max(1) };
        let rent = if rental { RENT_PER_ANTE } else { 0.0 };
        let back = sold.map_or(0, |i| run.jokers[i].sell_value) as f64;
        // (over the ante or so it's held: its boss's raise a third of the time)
        let earns = Joker::from_key(key, data).map_or(0.0, |j| 3.0 * self.added_money(&|k| Some(k) != sold, &j, self.antes_left.min(1.0)));
        let held = self.fill_long(self.project(&|k| Some(k) != sold, run.dollars), None, self.once(earns - ((cost - sell) as f64) - rent + back));
        self.ratio(self.long_score(&held), sold)
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
    /// joker that spends your first hand costs (DNA); your Blue Seals seen in fewer cards too
    fn hand_cost(&self, b: &Board) -> f64 {
        self.long_score_more(b, (0, -1, 0), 0) / self.long_score(b)
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
        let run = self.run;
        let back = sell.map_or(0, |i| run.jokers[i].sell_value) as f64;
        let rent = if rental { RENT_PER_ANTE } else { 0.0 };
        let horizon = if j.key == "j_madness" { 1.0 } else { self.antes_left };
        // used up by Ante 8 (`Growths`): its slot is filled like any empty one
        let g = self.growths.pool_after(j, self.growths.rounds).unwrap_or_else(|| Some(grow_antes(j, horizon).map_or_else(|| j.clone(), |g| g.0)));
        let b = self.fill_long(self.project_as(&|k| Some(k) != sell, run.dollars, rent, None, Some(j), (0, 0, 0)), g, self.once(back - cost as f64));
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

    /// Your projected board with `g` added (what it adds to a round, `Gain::round`, and its
    /// consumable slots are in the round it's played in: `value`)
    pub fn board_with(&self, g: &Gain) -> Board {
        let p = self.project_as(&|_| true, self.run.dollars + g.held, 0.0, g.joker.as_ref().map(|(i, j)| (*i, j)), None, g.round());
        let mut b = self.fill_long(p, None, self.once(g.money));
        if g.all_levels != 0 {
            for l in b.levels.iter_mut() {
                *l = add_levels(*l, g.all_levels as f64);
            }
        }
        Self::add_planets(&mut b, &g.planets, g.other_planets);
        for (ev, n) in g.events() {
            if n != 0.0 {
                b.after(ev, n);
            }
        }
        b
    }

    /// What `g` makes your run worth by Ante 8, as a ratio of your board as it is
    pub fn value(&self, g: &Gain) -> f64 {
        self.long_score_more(&self.board_with(g), g.round(), g.consumable_slots) / self.l0
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
        b.count_deck(d);
        let mut sp = self.long_spec_for(&b, (0, 0, 0));
        sp.start.deck = d.to_vec();
        (b, sp.start)
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
        let base = self.deck_base_on(rounds);
        self.value_against(d, money_once, rounds, 0, &base, None)
    }

    /// Your deck projected on the first `rounds` rounds (`deck_base`'s when they're as many)
    fn deck_base_on(&self, rounds: usize) -> crate::sim::Stats {
        if rounds == TAROT_ROUNDS {
            return *self.deck_base();
        }
        let cached = self.deck_base_n.lock().unwrap().get(&rounds).copied();
        cached.unwrap_or_else(|| {
            let st = self.deck_stats_n(&self.ctx.fresh_deck, self.run.dollars, 0.0, 0.0, rounds);
            self.deck_base_n.lock().unwrap().insert(rounds, st);
            st
        })
    }

    /// One of `parts` random outcomes of a change (outcome `part`): valued on its own slice of
    /// rounds (`part`·`rounds` onwards), against your deck on the rounds the outcomes cover
    /// together (`parts`·`rounds`), as many independent rounds as one projection on them.
    pub fn deck_value_part(&self, d: &[Card], money_once: f64, rounds: usize, part: usize, parts: usize) -> f64 {
        let base = self.deck_base_on(rounds * parts);
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
                if extra.abs() > MONEY_NOISE {
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
        let Some(&round) = self.round.get() else { return 0.0 };
        let slots = self.consumable_slots(0);
        // each deck at its own size: a deck change that removes cards draws your Blue Seals more
        self.seal_planets_in(d, slots, round) - self.seal_planets_in(&self.ctx.fresh_deck, slots, round)
    }

    /// Your consumable slots by Ante 8, `more` added (a projected quantity, as hands and hand
    /// size are: `long_spec_for`): today's, less the one each Negative consumable you hold adds
    /// (it goes when the card does: card.lua `Card:remove`, `queue_negative_removal`)
    pub(super) fn consumable_slots(&self, more: i64) -> i64 {
        let negative = self.run.consumables.iter().filter(|c| c.edition == Some(crate::model::Edition::Negative)).count() as i64;
        (self.run.consumable_slots - negative + more).max(0)
    }

    /// The planets deck `d`'s Blue Seal cards make in a round with `slots` consumable slots and
    /// `round` (hand size, hands, discards: the cards they're seen in), the consumables held at
    /// round end (`HELD_AT_ROUND_END`, a mix of the whole counts either side of it) taking
    /// theirs: `seal_planets_a_round` on what's left
    fn seal_planets_in(&self, d: &[Card], slots: i64, round: (i64, i64, i64)) -> f64 {
        let blue = d.iter().filter(|c| c.seal == Some(crate::model::Seal::Blue)).count();
        let lo = HELD_AT_ROUND_END.floor();
        let at = |held: f64| seal_planets_a_round(round, blue, d.len(), (slots as f64 - held).max(0.0) as usize);
        let w = HELD_AT_ROUND_END - lo;
        (1.0 - w) * at(lo) + if w > 0.0 { w * at(lo + 1.0) } else { 0.0 }
    }

    /// The planets your Blue Seal cards make by Ante 8 on board `b` with `more` hand size,
    /// hands and discards a round and `slots` consumable slots more (a `Gain`'s), against your
    /// board: a board whose jokers make the round bigger or smaller (Juggler, Stuntman…) sees
    /// them in more or fewer cards (3 rounds an ante; 0 when the round and slots are yours)
    pub(super) fn seal_planets_for(&self, b: &Board, more: (i64, i64, i64), slots: i64) -> f64 {
        self.seal_planets_in_round(slots, &self.long_spec_for(b, more), b)
    }

    /// `seal_planets_for` on the round `sp` starts with, played on `b`
    fn seal_planets_in_round(&self, slots: i64, sp: &Spec, b: &Board) -> f64 {
        let Some(&yours) = self.round.get() else { return 0.0 };
        let (_, s) = b.at_blind_set(&sp.start);
        let round = (s.hand_size, s.hands, s.discards);
        if slots == 0 && round == yours {
            return 0.0;
        }
        let d = &self.ctx.fresh_deck;
        (self.seal_planets_in(d, self.consumable_slots(slots), round) - self.seal_planets_in(d, self.consumable_slots(0), yours)) * 3.0 * self.antes_left
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
        let base = *self.deck_base();
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
                if extra.abs() > MONEY_NOISE { (rounds(extra), extra) } else { (r, 0.0) }
            }
        };
        (r.iter().map(|x| x.total.max(1.0) / base.mean.max(1.0)).collect(), used)
    }

    /// Your deck as it is, projected the same way: the baseline deck changes are compared to
    pub fn deck_base(&self) -> &crate::sim::Stats {
        self.deck_base.get_or_init(|| self.deck_stats(&self.ctx.fresh_deck, self.run.dollars, 0.0, 0.0))
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

/// The least chance of getting through the blinds ahead the board every change is measured on
/// (and placed at) is brought to (`Ahead`'s find): the shops'
/// estimate (`advise.rs` `survival_with`) counts only the best joker money finds, so a board it
/// still leaves near none meets the blinds stronger than that (planets, packs, tarots), and
/// against a chance near none any change would be worth many rounds (with the rule of three's
/// 3 wins in the runs instead, the same change was worth 9× more on 300 runs than on 60).
/// Hand-set, in the register.
pub(super) const AHEAD_FLOOR: f64 = 0.1;

/// The most sets of chained runs one search sizing the shops' find takes (`Ahead::find`,
/// `size_find`), and the most one search multiplies a score by: a budget, in the register. Within two standard
/// errors of the chance it's sized to (on the runs' count) it stops early.
const FIND_STEPS: usize = 6;
const FIND_MAX: f64 = 1000.0;

/// The most values of a joker's counter measured in the blinds ahead (`Ahead::counter`; the
/// rest read between them): a budget, in the register. Loyalty Card's cycle (6) and Seltzer's
/// hands left (at most the round's hands) fit; Yorick's discards (up to 23) are read between.
pub(super) const COUNTER_POINTS: usize = 8;

/// What the board a round leaves is worth in the blinds still ahead in this ante (`Ctx::ahead`:
/// the Big Blind's if it's still to come (taken as played, not skipped), then the boss's), for
/// Best play's round. A choice is worth its chance to get through times the long-run value of
/// what it leaves you: each goal's change is the shift of the log-odds it gives (`shift_of`),
/// measured on your board carrying what the shops before each blind find (`find`: a multiplier
/// on every score, sized so your board gets through as often as the shop ranking's estimate
/// says, at least `AHEAD_FLOOR`), so every change is measured at the chance it's placed at
/// (`reference`, the find's measured chance): `sim::RoundGoals::value` adds the shifts of what
/// a round leaves and takes the chance they give against the reference, so several jokers'
/// changes stay between none and a sure pass, as the joint runs do (measured 2026-10-18, gap
/// 18). Each board is played through
/// those blinds one after another on the same runs (`sim::chained_runs`, by the simulated
/// player without goals, as the blind odds), so a joker's numbers carry from the Big Blind into
/// the boss as its rounds leave them. A change of your board counts only where the paired runs
/// show it either way (`compare::better_by`: the paired difference less 2 standard errors), else
/// it's nothing; money's is the shops' estimate's, which the runs size (`dollar`). The next ante isn't counted (your board meets it after shops Best play doesn't
/// model).
pub(super) struct Ahead<'a> {
    ctx: &'a Ctx<'a>,
    /// Your board's runs as it is (1: got through), once a goal needs them
    plain: OnceLock<Vec<f64>>,
    /// The shops' find and your board's runs carrying it (`find`)
    find: OnceLock<Find>,
    /// A dollar's shift (`dollar`), once a goal needs it
    dollar: OnceLock<f64>,
    /// For each blind ahead, what the shops before it add to the boss's chance with the money you
    /// hold (the shop ranking's survival, `advise.rs` `survival_with`, against the boss's chance
    /// as it is, with that many shops: one before the first blind ahead, one more before each
    /// after it), never less than the blind's before; none where no shop pool is known (the
    /// quick pass)
    lifts: Vec<f64>,
    /// The same with $10 more (`dollar`)
    lifts_10: Vec<f64>,
}

/// The shops' find (`Ahead::find`): what it multiplies each blind's scores by (`at`), your
/// board's runs through the blinds ahead carrying it (`runs`), for each blind the boss's runs
/// carrying the find for the shops before it and the points the search that put them there
/// tried (`stages`; a shop that adds nothing keeps the one before's), and the multiplier the
/// floor added on every blind and its search's points (`floor`, `floor_curve`): where the find
/// for $10 more starts (`find_10`)
struct Find {
    at: Vec<f64>,
    runs: Vec<f64>,
    stages: Vec<(Vec<f64>, Option<Curve>)>,
    floor: f64,
    floor_curve: Option<Curve>,
}

/// The points a search (`size_find`) tried, each (log of the multiplier, log-odds of the chance)
/// from where it started, the one it kept (`kept`, its log) and the log-odds it was after (`goal`)
#[derive(Clone, Debug)]
struct Curve {
    tried: Vec<(f64, f64)>,
    kept: f64,
    goal: f64,
}

impl Curve {
    /// The slope of the log-odds against the log of the multiplier across the search's last
    /// bracket (the highest tried short of its goal and the lowest past it); none without one
    fn bracket(&self) -> Option<f64> {
        let short = self.tried.iter().filter(|t| t.1 < self.goal).max_by(|a, b| a.0.total_cmp(&b.0))?;
        let past = self.tried.iter().filter(|t| t.1 >= self.goal && t.0 > short.0).min_by(|a, b| a.0.total_cmp(&b.0))?;
        Some((past.1 - short.1) / (past.0 - short.0)).filter(|s| *s > 1e-9)
    }

    /// `bracket`, else the slope between the one kept and the one tried nearest it (a seed for a
    /// search, not a reading); none where they show no rise
    fn slope(&self) -> Option<f64> {
        self.bracket().or_else(|| {
            let kept = self.tried.iter().find(|t| t.0 == self.kept)?;
            let near = self.tried.iter().filter(|t| t.0 != self.kept).min_by(|a, b| (a.0 - self.kept).abs().total_cmp(&(b.0 - self.kept).abs()))?;
            Some((near.1 - kept.1) / (near.0 - kept.0)).filter(|s| *s > 1e-9)
        })
    }

    /// How much further (in the log of the multiplier) than the one kept, at log-odds `from`
    /// there, the log-odds reach `goal`, read off the points tried: between the one kept and the
    /// first tried past `goal` (so never past a multiplier the search saw get there), else by the
    /// slope across its last bracket; none where neither is there (it stopped short of its goal
    /// or on flat ground: what's read past its points then rests on a run or two)
    fn raise(&self, from: f64, goal: f64) -> Option<f64> {
        if goal <= from {
            return Some(0.0);
        }
        let past = self.tried.iter().filter(|t| t.0 > self.kept && t.1 >= goal).min_by(|a, b| a.0.total_cmp(&b.0));
        match past {
            Some(&(u, l)) => Some((goal - from) / (l - from) * (u - self.kept)),
            None => self.bracket().map(|s| (goal - from) / s),
        }
    }
}

/// The boss's chance as it is, and its chance by the money you'd hold with that many shops
/// before it (`Ahead::new`)
pub(super) type ShopsWith<'b> = (f64, &'b dyn Fn(f64, usize) -> f64);

impl<'a> Ahead<'a> {
    /// `None` with no blind ahead in this ante (outside a blind, or in the boss). `shops`: the
    /// boss's chance as it is and its chance by the money you'd hold with that many shops before
    /// it (`None`: no shop pool known, the quick pass: the shops add nothing counted there); in a
    /// blind, one shop comes before each blind ahead (the boss's are all of them)
    pub fn new(ctx: &'a Ctx<'a>, shops: Option<ShopsWith>) -> Option<Ahead<'a>> {
        if ctx.ahead.is_empty() {
            return None;
        }
        let d = ctx.run.dollars;
        let k = ctx.ahead.len();
        let lifts = |m: f64| {
            let mut l: Vec<f64> = (1..=k).map(|j| shops.map_or(0.0, |(p, with)| (with(m, j) - p).max(0.0))).collect();
            for j in 1..k {
                l[j] = l[j].max(l[j - 1]);
            }
            l
        };
        Some(Ahead { ctx, plain: OnceLock::new(), find: OnceLock::new(), dollar: OnceLock::new(), lifts: lifts(d), lifts_10: lifts(d + 10.0) })
    }

    fn plain(&self) -> &[f64] {
        self.plain.get_or_init(|| self.runs_at(&self.ctx.base, &[], None, &vec![1.0; self.ctx.ahead.len()]))
    }

    /// The shops' find: what the shops before each blind add to the boss's chance with the money
    /// you hold (`lifts`, the shop ranking's estimate, as it adds money's survival to an option's
    /// chance: `advise.rs` `rank_options`), put on the board as a multiplier on every score in
    /// the blinds after those shops (each blind's target divided by it: every score ×x, which
    /// takes no slot and no joker reads), sized shop by shop where the estimate is measured, on
    /// the boss alone from your board as it is (`boss_runs`): the first shop's on every blind
    /// ahead, so the boss carrying it gets through as often as with the first shop's lift, then
    /// each next shop's on top of it from the blind after that shop on, to that many shops' lift
    /// (at most what the runs can tell from a sure pass, `shops_target`): a search over its log
    /// against the chance's log-odds (`size_find`, within 2 standard errors of the chance), none
    /// where the boss already gets through that often, and none for a shop that adds nothing to
    /// the ones before it. So a Small Blind's Big Blind carries one shop's find and the boss
    /// two, and a Big Blind that holds the chain back doesn't push the boss's find up. Then,
    /// where your board carrying them gets through these blinds less often than `AHEAD_FLOOR`,
    /// every blind's find is raised by one more multiplier so it does
    fn find(&self) -> &Find {
        self.find.get_or_init(|| {
            let (n, k) = (self.n(), self.ctx.ahead.len());
            let base = &self.ctx.base;
            let mut at = vec![1.0; k];
            // the boss alone from your board as it is (with one blind ahead, the same runs as
            // the chain's); not needed where the shops add nothing counted, with the money held
            // or $10 more (the quick pass)
            let counted = self.lifts.iter().chain(&self.lifts_10).any(|&l| l > 0.0);
            let mut boss = if !counted { vec![] } else if k == 1 { self.plain().to_vec() } else { self.boss_runs(1.0) };
            let alone = mean(&boss);
            let mut stages = Vec::with_capacity(k);
            // the points of the search that put the boss's runs where they are (a search that
            // found it there already, or none, leaves them as they were)
            let mut curve = None;
            for j in 0..k {
                if counted && (j == 0 || self.lifts[j] > self.lifts[j - 1]) {
                    let t = shops_target(alone, self.lifts[j], n);
                    let (x, r, c) = size_find(t, 2.0 * se(t, n), n, None, boss, &|x| self.boss_runs(at[k - 1] * x));
                    at[j..].iter_mut().for_each(|m| *m *= x);
                    boss = r;
                    curve = c.or(curve);
                }
                stages.push((boss.clone(), curve.clone()));
            }
            let chain = if at.iter().all(|&m| m == 1.0) {
                self.plain().to_vec()
            } else if k == 1 {
                boss
            } else {
                self.runs_at(base, &[], None, &at)
            };
            let (floor, runs, floor_curve) = size_find(AHEAD_FLOOR, 2.0 * se(AHEAD_FLOOR, n), n, None, chain, &|x| self.runs_at(base, &[], None, &at.iter().map(|m| m * x).collect::<Vec<f64>>()));
            at.iter_mut().for_each(|m| *m *= floor);
            Find { at, runs, stages, floor, floor_curve }
        })
    }

    /// The multipliers on two or more blinds ahead of the shops' find with $10 more (`dollar`),
    /// and your board's runs carrying them, from the money held's (`find`): each blind's
    /// multiplier raised so the boss, carrying the money held's find for as many shops as come
    /// before that blind, would get through as much more often as $10 more adds to those shops'
    /// lift (`lifts_10` less `lifts`, the shop ranking's estimate) from where that find put it
    /// (the boss's chance it landed at: the difference of the lifts, not the lift with $10 more
    /// on its own, which against the search's 2 standard errors takes a typical $10 lift of
    /// 0.03-0.05 to nothing or twice over), at most what the runs tell from a sure pass
    /// (`shops_target`); never less on a blind than on the one before it (it carries every shop
    /// before it). Each read off the points the search that put the boss there tried
    /// (`Curve::raise`: up to the first past that chance, else the slope across its last
    /// bracket), so the find for $10 more comes from the same search; where they don't reach it
    /// (none tried, as the boss already got through that often, or no bracket) or `AHEAD_FLOOR`
    /// raised the find, by a search of its own (`size_find`, within one standard error of the
    /// difference on the same runs, at least a run; from those points' slope, or the floor's
    /// where it binds, unseeded where there are none) on the boss carrying the money held's
    /// find, the floor's too (a board the shops' find left near none barely moves on the boss
    /// alone: the multiplier that moves it a little there moves the floor's board a lot)
    fn find_10(&self) -> (Vec<f64>, Vec<f64>) {
        let f = self.find();
        let n = self.n();
        let lo = |p: f64| crate::sim::log_odds(p, n);
        let mut at = f.at.clone();
        for (j, m) in at.iter_mut().enumerate() {
            let d = self.lifts_10[j] - self.lifts[j];
            if d <= 0.0 {
                continue;
            }
            let (runs, curve) = &f.stages[j];
            let p = mean(runs);
            let read = curve.as_ref().filter(|_| f.floor == 1.0).and_then(|c| c.raise(lo(p), lo(shops_target(p, d, n).max(p))));
            *m *= match read {
                Some(u) => u.max(0.0).exp(),
                None => {
                    let start = if f.floor == 1.0 { runs.clone() } else { self.boss_runs(f.at[j]) };
                    let target = shops_target(mean(&start), d, n);
                    let seed = if f.floor == 1.0 { curve.as_ref() } else { f.floor_curve.as_ref() }.and_then(Curve::slope);
                    size_find(target, se(d.min(1.0), n).max(1.0 / n.max(1) as f64), n, seed, start, &|x| self.boss_runs(f.at[j] * x)).0
                }
            };
        }
        for j in 1..at.len() {
            at[j] = at[j].max(at[j - 1]);
        }
        let runs = if at == f.at { f.runs.clone() } else { self.runs_at(&self.ctx.base, &[], None, &at) };
        (at, runs)
    }

    /// The multipliers of the shops' find with $10 more (`find_10`) and your board's chance
    /// through the blinds ahead carrying them
    #[cfg(test)]
    pub(super) fn find_10_at(&self) -> (Vec<f64>, f64) {
        let (at, runs) = self.find_10();
        (at, mean(&runs))
    }

    /// The multipliers the shops' find puts on each blind ahead (`find`), and your board's
    /// chance through them carrying it
    #[cfg(test)]
    pub(super) fn find_at(&self) -> (Vec<f64>, f64) {
        let f = self.find();
        (f.at.clone(), mean(&f.runs))
    }

    /// Your board's runs carrying the shops' find: what every change is measured against
    fn now(&self) -> &[f64] {
        &self.find().runs
    }

    /// The runs each chance is measured on (`log_odds`' half a run)
    fn n(&self) -> usize {
        self.ctx.opts.sims
    }

    /// The chance every change is placed at: your board's through the blinds ahead carrying the
    /// shops' find (`find`), the chance every change was measured at
    fn reference(&self) -> f64 {
        mean(self.now())
    }

    /// The log-odds of `reference` (`sim::RoundGoals::ahead`)
    pub fn base(&self) -> f64 {
        crate::sim::log_odds(self.reference(), self.n())
    }

    /// What a shift is worth as a share of a round (the chance it gives against `reference`, less 1)
    pub fn share(&self, shift: f64) -> f64 {
        crate::sim::chance_ratio(self.base(), shift, 0.0) - 1.0
    }

    /// A dollar more at the round's end: what $10 more buys in the shops before each blind
    /// (`lifts_10` against `lifts`), as the shift of the log-odds through the blinds ahead between
    /// your board carrying the shops' find for $10 more and the money held's (`find`), a tenth of
    /// it; never below nothing. With one blind ahead, the boss's chance with $10 more is where
    /// the find put it plus what $10 more adds to the shops' lift (at most what the runs tell
    /// from a sure pass), as a find sized to that would get through (a search for it only adds
    /// its tolerance); with two, measured on the runs (`find_10`), as the Big Blind's find is sized
    /// on the boss. Not gated by the paired runs (`shift_of`): that $10 adds to the shops' lift
    /// is the shop ranking's estimate, gated by its own rule of three (`advise.rs`
    /// `measured_p`), and the runs measure how much it moves the blinds ahead where it's placed
    /// (on the same runs, a lift of 0.04 the find put on the boss was taken to nothing about half
    /// the time, as the simulated player's runs part both ways)
    pub fn dollar(&self) -> f64 {
        *self.dollar.get_or_init(|| {
            if self.lifts_10.iter().zip(&self.lifts).all(|(t, l)| t <= l) {
                return 0.0;
            }
            let (n, k) = (self.n(), self.ctx.ahead.len());
            let lo = |p: f64| crate::sim::log_odds(p, n);
            let now = self.reference();
            let with = if k == 1 { shops_target(now, self.lifts_10[0] - self.lifts[0], n).max(now) } else { mean(&self.find_10().1) };
            ((lo(with) - lo(now)) / 10.0).max(0.0)
        })
    }

    /// `b`'s runs through the blinds ahead carrying the shops' find (`find`; `removed`: jokers
    /// taken off your board, for the hands and discards they bring; `deck`: your deck changed)
    fn runs(&self, b: &Board, removed: &[&Joker], deck: Option<&[Card]>) -> Vec<f64> {
        self.runs_at(b, removed, deck, &self.find().at)
    }

    /// `runs` with every score in each blind multiplied by its `find`
    fn runs_at(&self, b: &Board, removed: &[&Joker], deck: Option<&[Card]>, find: &[f64]) -> Vec<f64> {
        let rounds: Vec<RoundStart> = self.ctx.ahead.iter().zip(find).map(|(spec, &x)| self.round(spec, removed, deck, x)).collect();
        self.chained(b, &rounds)
    }

    /// The boss alone (the last blind ahead) from your board as it is, every score ×`x`: where
    /// the shops' estimate is measured (`find`)
    fn boss_runs(&self, x: f64) -> Vec<f64> {
        let b = &self.ctx.base;
        let spec = self.ctx.ahead.last().expect("a blind ahead");
        self.chained(b, &[self.round(spec, &[], None, x)])
    }

    /// Blind `spec`'s round (as `runs`), every score ×`find`
    fn round(&self, spec: &Spec, removed: &[&Joker], deck: Option<&[Card]>, find: f64) -> RoundStart {
        let mut sp = spec.clone();
        if let Some(d) = deck {
            sp.start.deck = d.to_vec();
        }
        let mut start = super::apply_mods(&sp.start, &[], removed);
        start.target /= find;
        start
    }

    fn chained(&self, b: &Board, rounds: &[RoundStart]) -> Vec<f64> {
        crate::sim::chained_runs(b, rounds, self.ctx.opts.sims, self.ctx.opts.seed).into_iter().map(|w| if w { 1.0 } else { 0.0 }).collect()
    }

    /// Runs `with` against `against` (the same runs, both carrying the shops' find) as the shift
    /// of the log-odds between their chances, when the paired runs show them apart; nothing
    /// otherwise
    fn shift_of(&self, with: &[f64], against: &[f64]) -> f64 {
        use super::compare::better_by;
        if better_by(with, against, false) <= 0.0 && better_by(against, with, false) <= 0.0 {
            return 0.0;
        }
        let n = self.n();
        crate::sim::log_odds(mean(with), n) - crate::sim::log_odds(mean(against), n)
    }

    fn shift(&self, b: &Board, removed: &[&Joker]) -> f64 {
        self.shift_of(&self.runs(b, removed, None), self.now())
    }

    /// Your joker `i`'s Mult, ×Mult and Chips (the numbers `Joker::lasting` counts; only
    /// `fields`, the ones its rounds move, the rest nothing) in the blinds ahead
    /// (`sim::AheadWorth`, shifts): per unit above where they are, on the steps `joker_worth`
    /// takes (`GROWTH_STEPS`); below, per unit down a step of the same size and past it down to
    /// none (Mult 0, ×1, Chips 0: a reset), as a blind's chance isn't linear in them (a flush
    /// board can take +2 Mult with its boss's chance unmoved and lose a quarter of it without the
    /// 7 it has); and losing it, at least what its numbers at none cost. Never below nothing per
    /// unit, and losing it never a gain.
    pub fn joker(&self, i: usize, fields: [bool; 3]) -> crate::sim::AheadWorth {
        let base = &self.ctx.base;
        let start = base.jokers[i].lasting();
        let none = [0.0, 1.0, 0.0];
        let set = |f: usize, to: f64| {
            let mut b = base.clone();
            let c = &mut b.jokers[i];
            match f {
                0 => c.mult = to,
                1 => c.x_mult = to,
                _ => c.extra.chips = to,
            }
            b
        };
        let mut w = crate::sim::AheadWorth { knot: std::array::from_fn(|f| (start[f] - GROWTH_STEPS[f]).max(none[f])), lost: true, ..Default::default() };
        for f in (0..3).filter(|&f| fields[f]) {
            w.up[f] = (self.shift(&set(f, start[f] + GROWTH_STEPS[f]), &[]) / GROWTH_STEPS[f]).max(0.0);
            if start[f] <= none[f] {
                continue;
            }
            let at_knot = self.shift(&set(f, w.knot[f]), &[]);
            w.near[f] = (at_knot / (w.knot[f] - start[f])).max(0.0);
            if w.knot[f] > none[f] {
                w.far[f] = ((self.shift(&set(f, none[f]), &[]) - at_knot) / (none[f] - w.knot[f])).max(0.0);
            }
        }
        // losing it costs at least what its numbers at none do (where its loss alone isn't shown
        // but a fall is, the fall still counts: `AheadWorth::of` floors at `gone`)
        w.gone = w.loss(self.shift(&super::without(base, i), &[&base.jokers[i]]), &start);
        w
    }

    /// Your joker `i`'s counter (`Joker::counter`: Loyalty Card's place in its cycle, Yorick's
    /// discards to its next step, Seltzer's hands left) at each value the round in play can leave
    /// it at (`Joker::counter_after`, with the hands and discards it has left: at most 5 cards a
    /// discard, the game's limit, and what the boss's press-play step takes a hand,
    /// `engine::hook_takes`), as the shift each gives against the counter where it is now
    /// (`shift`, on the same runs as every change; none where they don't show it): so the hands
    /// a round takes land Loyalty Card's ×Mult on a hand of the blinds ahead or off them all. At
    /// most `COUNTER_POINTS` values measured: every so many of them in the order the fewest
    /// hands and cards reach them, the first and the last included, so the nearest are always
    /// measured (Yorick's next discard, across its wrap too); the rest read between them by value
    /// (`sim::CounterWorth::of`). None where it has no counter or the round can't move it.
    pub fn counter(&self, i: usize) -> Option<crate::sim::CounterWorth> {
        let base = &self.ctx.base;
        let j = &base.jokers[i];
        let start = j.counter(base.hands_played)?;
        let (hands, discards) = self.ctx.specs.iter().find(|s| s.in_progress())?.start.hands_on(base);
        let cards = 5 * discards.max(0) + crate::engine::hook_takes(base, usize::MAX) as i64 * hands.max(0);
        // in the order the fewest moves (hands and cards) reach them, each once
        let mut reach: Vec<(i64, f64)> = (0..=hands.max(0)).flat_map(|h| (0..=cards).filter_map(move |c| j.counter_after(base.hands_played, h, c).map(|v| (h + c, v)))).filter(|&(_, v)| v != start).collect();
        reach.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut values: Vec<f64> = vec![];
        for (_, v) in reach {
            if !values.contains(&v) {
                values.push(v);
            }
        }
        if values.is_empty() {
            return None;
        }
        if values.len() > COUNTER_POINTS {
            let n = values.len() - 1;
            values = (0..COUNTER_POINTS).map(|k| values[k * n / (COUNTER_POINTS - 1)]).collect();
        }
        let shifts = par_map(&values, |&v| {
            let mut b = base.clone();
            b.jokers[i].set_counter(base.hands_played, v);
            self.shift(&b, &[])
        });
        let mut at: Vec<(f64, f64)> = values.into_iter().zip(shifts).chain([(start, 0.0)]).collect();
        at.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(crate::sim::CounterWorth { start, at })
    }

    /// One more level of hand `h` (a Blue Seal's planet, used as the simulated round uses one:
    /// Constellation's ×0.1 with it): its shift, never below nothing
    pub fn level(&self, h: HandType) -> f64 {
        let mut levels = [0; 12];
        levels[h as usize] = 1;
        let planet = crate::sim::Use { levels, planet: true, ..Default::default() };
        planet.apply(&self.ctx.base, &[]).map_or(0.0, |(b, _)| self.shift(&b, &[]).max(0.0))
    }

    /// The runs with your deck as `deck` (your fresh deck with a consumable used on a card)
    pub fn deck_runs(&self, deck: &[Card]) -> Vec<f64> {
        self.runs(&super::board_with_deck(&self.ctx.base, deck), &[], Some(deck))
    }

    /// Your deck as `deck` against the runs `against` (`deck_runs` of the deck it's weighed
    /// against: a consumable's card drawn against its target in your hand), as a share of a
    /// round (`share`), never below nothing
    pub fn deck(&self, deck: &[Card], against: &[f64]) -> f64 {
        self.share(self.shift_of(&self.deck_runs(deck), against)).max(0.0)
    }
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// `Ahead::find`'s chance for the boss: its chance as it is `alone`, plus what the shops add
/// (`lift`), at most what `n` runs tell from a sure pass (the rule of three's 3 runs short of
/// all, `advise.rs` `three_wins`: closer, the find would be sized to the hardest few runs)
fn shops_target(alone: f64, lift: f64, n: usize) -> f64 {
    (alone + lift).min(1.0 - super::three_wins(n))
}

/// The multiplier on every score (at least 1, at most `FIND_MAX`) whose runs (`runs_at(x)`,
/// `n` of them; `plain` at 1) get through closest to `target` (the smallest of those as close),
/// those runs, and the points it tried (`Curve`): none (1, no points) when `plain` is already
/// within `tol` of it or past it; else its log searched against the log-odds of the chance, a
/// step past the last one tried while none reached `target` (from ×2, or from the slope `seed`
/// (`Ahead::find_10`: the floor's), then each step's slope, at most ×10 a step and, unseeded, at
/// least ×1.1, ×10 where the step before moved nothing, two such steps ending it; seeded, a step
/// that moves nothing keeps the slope), then between the two either side of it (the secant, kept
/// a tenth inside them), at most `FIND_STEPS` sets of runs, stopping once within `tol`
/// (`Ahead::find`: two standard errors of the chance; `find_10`: one of a difference on paired
/// runs)
fn size_find(target: f64, tol: f64, n: usize, seed: Option<f64>, plain: Vec<f64>, runs_at: &dyn Fn(f64) -> Vec<f64>) -> (f64, Vec<f64>, Option<Curve>) {
    let close = |p: f64| (p - target).abs() <= tol;
    let p0 = mean(&plain);
    if p0 >= target || close(p0) {
        return (1.0, plain, None);
    }
    let lo = |p: f64| crate::sim::log_odds(p, n);
    let goal = lo(target);
    // (log of the multiplier, log-odds): the highest tried below the target and the lowest at
    // or past it; the closest tried, with its runs
    let mut below = (0.0, lo(p0));
    let mut above: Option<(f64, f64)> = None;
    let mut best = (0.0, plain);
    let mut slope = seed.filter(|&s| s > 1e-9);
    let mut flat = 0;
    // every (log of the multiplier, log-odds) tried, for where it landed (`Curve`)
    let mut tried = vec![below];
    for _ in 0..FIND_STEPS {
        let u = match above {
            Some(a) => {
                let t = ((goal - below.1) / (a.1 - below.1).max(1e-9)).clamp(0.1, 0.9);
                below.0 + t * (a.0 - below.0)
            }
            None => {
                // a seeded search's steps may be as small as their slope says (a $10 move is
                // often less than ×1.1)
                let least = if seed.is_some() { 0.0 } else { 0.1 };
                let step = slope.map_or(2f64.ln(), |s| if s > 1e-9 { (goal - below.1) / s } else { 10f64.ln() }).clamp(least, 10f64.ln());
                (below.0 + step).min(FIND_MAX.ln())
            }
        };
        let runs = runs_at(u.exp());
        let p = mean(&runs);
        let l = lo(p);
        tried.push((u, l));
        let (d, d_best) = ((p - target).abs(), (mean(&best.1) - target).abs());
        let done = close(p);
        if d < d_best || (d == d_best && u < best.0) {
            best = (u, runs);
        }
        if done {
            break;
        }
        if p >= target {
            above = Some((u, l));
        } else {
            // a seeded search's small steps often move no run: its slope stands and it steps
            // again, where an unseeded one takes ×10 and stops after two
            let rose = l > below.1;
            flat = if !rose && above.is_none() { flat + 1 } else { 0 };
            if u > below.0 && (rose || seed.is_none()) {
                slope = Some((l - below.1) / (u - below.0));
            }
            let top = u >= FIND_MAX.ln();
            below = (u, l);
            if top || (flat >= 2 && seed.is_none()) {
                break;
            }
        }
    }
    (best.0.exp(), best.1, Some(Curve { tried, kept: best.0, goal }))
}

/// The standard error of a chance `p` measured on `n` runs
fn se(p: f64, n: usize) -> f64 {
    (p * (1.0 - p) / n.max(1) as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_find_is_sized_to_the_shops_and_a_shift_stays_between_none_and_a_sure_pass() {
        // the find is sized to the boss's chance as it is, plus what the shops before it add
        assert!((shops_target(0.4, 0.5, 300) - 0.9).abs() < 1e-12);
        // at most what 300 runs tell from a sure pass
        assert_eq!(shops_target(0.7, 0.5, 300), 0.99);
        // a shift measured on the runs, from 0.4 to 0.1, placed there: at 0.4 itself the same
        // 0.1, at 0.9 the same odds' ratio (1/6), never past a sure pass, and a loss of
        // everything stays finite
        let lo = |p: f64| crate::sim::log_odds(p, 300);
        let s = lo(0.1) - lo(0.4);
        assert!((crate::sim::chance_ratio(lo(0.4), s, 0.0) - 0.25).abs() < 1e-9);
        assert!((crate::sim::chance_ratio(lo(0.9), s, 0.0) * 0.9 - 0.6).abs() < 1e-9);
        assert!(crate::sim::chance_ratio(lo(0.9), 50.0, 0.0) <= 1.0 / 0.9 + 1e-12);
        assert!((lo(0.0) - lo(0.5)).is_finite());
    }

    #[test]
    fn the_shops_find_is_sized_to_the_chance_it_stands_for() {
        // runs that get through as often as log-odds −3 + 1.5 ln x says (the first of them)
        let n = 300;
        let runs = |x: f64| {
            let won = (crate::sim::chance_at(-3.0 + 1.5 * x.ln()) * n as f64).round() as usize;
            (0..n).map(|i| if i < won { 1.0 } else { 0.0 }).collect::<Vec<f64>>()
        };
        let tries = std::cell::Cell::new(0);
        let counted = |x: f64| {
            tries.set(tries.get() + 1);
            runs(x)
        };
        // already there or past it: no find, no runs
        let (x, r, _) = size_find(0.04, 2.0 * se(0.04, n), n, None, runs(1.0), &counted);
        assert_eq!((x, tries.get()), (1.0, 0));
        assert_eq!(r, runs(1.0));
        // a find that lifts 4.7% to 60%: within two standard errors, in a few sets of runs
        let (x, r, _) = size_find(0.6, 2.0 * se(0.6, n), n, None, runs(1.0), &counted);
        assert!((mean(&r) - 0.6).abs() <= 2.0 * se(0.6, n), "{x} {}", mean(&r));
        assert!(tries.get() <= FIND_STEPS && x > 1.0);
        assert_eq!(r, runs(x));
        // a board no find lifts: two steps that move nothing, and no find
        tries.set(0);
        let (x, r, _) = size_find(0.5, 2.0 * se(0.5, n), n, None, vec![0.0; n], &|x| {
            tries.set(tries.get() + 1);
            vec![0.0; n].into_iter().map(|v| v + 0.0 * x).collect()
        });
        assert!(x == 1.0 && tries.get() == 2 && mean(&r) == 0.0);
        // one that a find lifts only so far (to 30%): the smallest multiplier that gets there
        let capped = |x: f64| {
            let won = (crate::sim::chance_at(-3.0 + 1.5 * x.ln()).min(0.3) * n as f64).round() as usize;
            (0..n).map(|i| if i < won { 1.0 } else { 0.0 }).collect::<Vec<f64>>()
        };
        let (x, r, _) = size_find(0.8, 2.0 * se(0.8, n), n, None, capped(1.0), &capped);
        assert!(mean(&r) == 0.3 && x < 100.0, "{x}");
        // a steep board, past the chance at ×2 (from 2.5% to 90%): back between the two, the
        // closest kept
        let steep = |x: f64| {
            let won = (crate::sim::chance_at(-3.7 + 9.0 * x.ln()) * n as f64).round() as usize;
            (0..n).map(|i| if i < won { 1.0 } else { 0.0 }).collect::<Vec<f64>>()
        };
        tries.set(0);
        let (x, r, _) = size_find(0.3, 2.0 * se(0.3, n), n, None, steep(1.0), &|x| {
            tries.set(tries.get() + 1);
            steep(x)
        });
        assert!(x > 1.0 && x < 2.0 && tries.get() <= FIND_STEPS, "{x}");
        assert!((mean(&r) - 0.3).abs() < 0.1, "{}", mean(&r));
        assert_eq!(r, steep(x));
        // a board that gets through every run already, sized to a sure pass: no find
        tries.set(0);
        let (x, _, _) = size_find(shops_target(0.9, 0.5, n), 2.0 * se(0.99, n), n, None, vec![1.0; n], &counted);
        assert_eq!((x, tries.get()), (1.0, 0));
        // noisy runs that don't rise steadily: still within the budget, the runs at the one kept
        let noisy = |x: f64| {
            let p = (0.05 + 0.3 * x.ln() + 0.08 * (7.0 * x).sin()).clamp(0.0, 1.0);
            let won = (p * n as f64).round() as usize;
            (0..n).map(|i| if i < won { 1.0 } else { 0.0 }).collect::<Vec<f64>>()
        };
        tries.set(0);
        let (x, r, _) = size_find(0.5, 2.0 * se(0.5, n), n, None, noisy(1.0), &|x| {
            tries.set(tries.get() + 1);
            noisy(x)
        });
        assert!(tries.get() <= FIND_STEPS && (1.0..=FIND_MAX).contains(&x));
        assert_eq!(r, noisy(x));
        // a $10 move from where a find landed (30%, 4% more), seeded with the slope there: a
        // step of ×1.13 (unseeded, the first step would be ×2, to 55%), within one standard error
        // of the difference in a try or two
        let x0 = ((crate::sim::log_odds(0.3, n) + 3.0) / 1.5).exp();
        let start = runs(x0);
        let target = mean(&start) + 0.04;
        tries.set(0);
        let (x, r, curve) = size_find(target, se(0.04, n), n, Some(1.5), start, &|x| {
            tries.set(tries.get() + 1);
            runs(x0 * x)
        });
        assert!((mean(&r) - target).abs() <= se(0.04, n) && x > 1.0 && x < 1.2 && tries.get() <= 2, "{x} {} {}", mean(&r), tries.get());
        let slope = curve.as_ref().and_then(Curve::slope);
        assert!(slope.is_some_and(|s| (s - 1.5).abs() < 0.5), "{slope:?}");
        // and a $10 move read off where the search landed: the multiplier that model needs for
        // 4% more there, from the points either side of it
        let (c, p) = (curve.unwrap(), mean(&r));
        let want = (crate::sim::log_odds(p + 0.04, n) - crate::sim::log_odds(p, n)) / 1.5;
        let got = c.raise(crate::sim::log_odds(p, n), crate::sim::log_odds(p + 0.04, n)).unwrap();
        assert!((got - want).abs() < 0.5 * want, "{got} {want}");
        // a search's points past the chance asked for bound how far it's read (the point tried
        // at 0.7, where the slope across its last bracket would read 0.42)
        let c = Curve { tried: vec![(0.0, -1.0), (0.3, -0.5), (0.7, 0.2)], kept: 0.3, goal: -0.5 };
        assert!((c.raise(-0.5, 0.2).unwrap() - 0.4).abs() < 1e-12);
        assert!((c.raise(-0.5, -0.15).unwrap() - 0.2).abs() < 1e-12);
        assert_eq!(c.raise(-0.5, -0.6), Some(0.0));
    }

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
