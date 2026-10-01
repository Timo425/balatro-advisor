//! **Valuation**: what a board, a joker, money or a planet is worth by Ante 8, in one measure.
//!
//! Every board is projected to the end of the run (growers grown, faders faded, perishables
//! that run out gone, planets bought with the money it leaves you) and scored on whole
//! simulated rounds; values are ratios against your board as it is (`l0`). Money is the one
//! channel economy flows through, in two forms: money you hold every ante (rent lowers it)
//! sets how many pack skips, rerolls and planets you buy each ante; one-off money (a price, a
//! sell-back, Temperance) buys them once. Nothing is valued twice.

use std::sync::OnceLock;

use super::{grow_antes, income_per_ante, interest, par_map, round_mods, seal_round_chance, Candidate, Ctx, HandShare, Spec};
use crate::data::GameData;
use crate::engine::{Board, Joker, Kind};
use crate::model::Card;
use crate::save::{JokerCard, RunState};
use crate::sim::{RoundRules, RoundStart};

/// Rent ($3 a round) comes out of the money that would buy planets, rerolls and pack skips:
/// each rental counts as one ante of rent less money held.
pub(super) const RENT_PER_ANTE: f64 = 9.0;

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
        let line = self.line;
        let below = if flow < 0.0 { -(-flow).min((line - dollars).max(0.0)) } else { flow.min((line - (dollars - flow)).max(0.0)) };
        let per_ante = (0.5 + (dollars - line).max(0.0) / 20.0 + below / 18.0).clamp(0.0, 2.0);
        (per_ante, per_ante * self.antes_left)
    }

    /// Money jokers you keep pay every ante, like rent in reverse.
    pub fn owned_income(&self, keep: &dyn Fn(usize) -> bool) -> f64 {
        let run = self.run;
        run.jokers.iter().enumerate().filter(|(i, sj)| keep(*i) && self.lasts(sj) && !sj.debuff).map(|(_, sj)| income_per_ante(&sj.key, &sj.ability, run, self.antes_left)).sum::<f64>()
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

    /// The projected board: the jokers kept (by index), grown with the money you'd hold.
    /// `extra_rent`: more rent per ante (negative: more income), on top of your jokers'.
    pub fn project_rent(&self, keep: &dyn Fn(usize) -> bool, dollars: f64, extra_rent: f64) -> Board {
        let (ctx, run) = (self.ctx, self.run);
        let rent = self.owned_rent(keep) + extra_rent;
        let flow = self.owned_income(keep) - rent;
        let dollars = dollars + flow;
        let mut b = ctx.base.clone();
        b.blind = Default::default();
        if let Some(top) = self.top_hand {
            let l = b.levels[top as usize];
            let n = self.levels_for(dollars, flow).1;
            let mut lv = l.with_level(l.level + n.floor() as i64);
            lv.chips += lv.l_chips * (n - n.floor());
            lv.mult += lv.l_mult * (n - n.floor());
            b.levels[top as usize] = lv;
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
                    grow_antes(j, self.hand_mix, dollars, self.line, self.antes_left).map_or_else(|| j.clone(), |g| g.0)
                }
            })
            .collect();
        b
    }

    pub fn project(&self, keep: &dyn Fn(usize) -> bool, dollars: f64) -> Board {
        self.project_rent(keep, dollars, 0.0)
    }

    /// One-off money: spent once on pack skips / rerolls for a joker that grows from them
    /// (about $5 each), else on planets for your main hand.
    pub fn spend_once(&self, b: &mut Board, once: f64) {
        // Pack skips / rerolls cost about $5 each; a level of your main hand about $12 (its
        // planet is only in some shops and packs: a Celestial pack holds it ~1 time in 4);
        // any planet about $4, and each one used grows Constellation by ×0.1.
        let buys = once / 5.0;
        for j in b.jokers.iter_mut().filter(|j| j.key == "j_constellation") {
            j.x_mult = (j.x_mult + 0.1 * once / 4.0).max(1.0);
        }
        let buys_main = once / 12.0;
        if let Some(j) = b.jokers.iter_mut().find(|j| j.key == "j_red_card" || j.key == "j_flash") {
            let per = if j.key == "j_red_card" { 3.0 } else { 2.0 };
            j.mult = (j.mult + per * buys).max(0.0);
        } else if let Some(top) = self.top_hand {
            let buys = buys_main;
            // Fractional levels (as their chips and mult): rounding made any small purchase
            // cost a whole level
            let l = b.levels[top as usize];
            let whole = buys.floor();
            let mut n = l.with_level((l.level + whole as i64).max(1));
            if n.level + 1 > 1 || buys - whole > 0.0 {
                let frac = buys - whole;
                n.chips += n.l_chips * frac;
                n.mult += n.l_mult * frac;
            }
            b.levels[top as usize] = n;
        }
    }

    /// The board with the option (or a typical find) in its slot, one-off money spent, and
    /// empty slots filled with stand-ins.
    pub fn fill_long(&self, mut b: Board, option: Option<Joker>, once: f64) -> Board {
        b.jokers.push(option.unwrap_or_else(|| self.stand_in(1.25, 0.0, 0.0)));
        self.spend_once(&mut b, once);
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

    /// DNA (card.lua: first hand of the round a single card → a permanent copy of it): a copy
    /// of your best card every round until Ante 8, your first hand spent on it, and the
    /// copies' Blue Seal planets (at most 2 a round: consumable slots).
    pub fn dna_long(&self, b: Board) -> f64 {
        use crate::model::{Enhancement, Seal};
        let (ctx, run) = (self.ctx, self.run);
        let deck = &ctx.fresh_deck;
        let rank = |c: &Card| match (c.seal, c.enhancement) {
            (Some(Seal::Blue), _) => 5,
            (Some(Seal::Red), Some(Enhancement::Glass)) => 4,
            (_, Some(Enhancement::Glass)) => 3,
            (Some(_), _) => 2,
            (_, Some(_)) => 1,
            _ => 0,
        };
        let Some(best) = deck.iter().copied().max_by_key(|c| (rank(c), c.rank.0)) else { return 1.0 };
        let rounds = (3.0 * self.antes_left).floor() as usize;
        // The card has to be in your opening hand to be the single first play: each round,
        // the chance that one of its copies is among the first `hand size` cards.
        let open = |s: f64, deck: f64| 1.0 - (1.0 - s / deck.max(1.0)).powf(run.hand_size as f64);
        let s0 = deck.iter().filter(|c| **c == best).count() as f64;
        let (mut copies, mut planets) = (s0, 0.0);
        for r in 0..rounds {
            let dsize = deck.len() as f64 + copies - s0;
            copies += open(copies, dsize);
            if best.seal == Some(Seal::Blue) {
                let _ = r;
                planets += (2.0f64).min(copies * seal_round_chance(run, dsize as usize)) - (2.0f64).min(s0 * seal_round_chance(run, deck.len()));
            }
        }
        let added = (copies - s0).round() as usize;
        let mut d = deck.clone();
        d.extend(std::iter::repeat_n(best, added));
        let mut b = b;
        let tally = |e: Enhancement| d.iter().filter(|c| c.enhancement == Some(e)).count() as i64;
        b.steel_tally = tally(Enhancement::Steel);
        b.stone_tally = tally(Enhancement::Stone);
        b.driver_tally = d.iter().filter(|c| c.enhancement.is_some()).count() as i64;
        b.playing_cards = d.len() as i64;
        if best.seal == Some(Seal::Blue) {
            let extra = planets.max(0.0);
            for j in b.jokers.iter_mut().filter(|j| j.key == "j_constellation") {
                j.x_mult += 0.1 * extra;
            }
            if let Some(top) = self.top_hand {
                let l = b.levels[top as usize];
                let mut lv = l.with_level(l.level + extra.floor() as i64);
                lv.chips += lv.l_chips * (extra - extra.floor());
                lv.mult += lv.l_mult * (extra - extra.floor());
                b.levels[top as usize] = lv;
            }
        }
        let mut sp = self.long_spec_for(&b);
        sp.start.deck = d;
        sp.start.hands = (sp.start.hands - 1).max(1); // the first hand goes on the single card
        ctx.odds_one(&b, &sp, 48).1.mean.max(1.0)
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
        let g = grow_antes(j, self.hand_mix, run.dollars + income - rent, self.line, horizon).map_or_else(|| j.clone(), |g| g.0);
        let b = self.fill_long(self.project_rent(&|k| Some(k) != sell, run.dollars, rent - income), Some(g), self.once(back - cost as f64));
        if j.key == "j_dna" { self.dna_long(b) } else { self.long_score(&b) }
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
