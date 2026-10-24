//! A shop option played on against leaving the shop, for judging what the shop ranking says
//! about spending (gap 17 in docs/design.md, measured with it 2026-10-24). From a saved shop
//! state: the option taken (a joker from the shop, a reroll, a Buffoon pack) or not, then the
//! shops up to the next ante's boss played by one fixed policy in both branches, the same
//! shop cards for both (paired), each board's chances at this ante's boss and the next ante's
//! plain boss simulated by `whatif` (the engine's rounds), money with the game's interest and
//! the blinds' rewards. It shares the engine with the advice, not its valuation: the jokers are
//! held, not valued; each board picked on one set of rounds and scored on another, 400
//! rounds, so a pick's own noise isn't counted as its gain. The policy is a heuristic: in each shop, buy the joker that most raises
//! the boss's chance ahead (this ante's × the next ante's before this boss), in a free slot or
//! for the joker whose sale leaves the most, rerolling up to 3 times; "spend" buys and rerolls
//! while it can pay, "cap" keeps the interest cap (buys below it only for 5 points or more,
//! rerolls only above it). Not counted: editions, packs, planets and tarots in later shops,
//! vouchers, skips, rent, the hands' money beyond $1 a round, anything after the next ante's
//! boss (the money left there is printed). Slow (a board with full slots tries every swap):
//! `BAV_PLAYON=list.txt cargo test --release --test shop_playon -- --ignored --nocapture`, a
//! list of lines `<state.jkr> <option>`: `joker:<key>`, `reroll`, `pack:<jokers>:<price>`;
//! `BAV_PLAYON_TRIALS` (24), `BAV_PLAYON_SIMS` (150).

use std::collections::HashMap;
use std::sync::Mutex;

use balatro_advisor::{advise, data::GameData, save, whatif};

/// The rounds a branch's boards are scored on
const SCORE_SIMS: usize = 400;
/// The side stream a held joker is drawn again on
const SIDE: u64 = 0x5eed_51de;

/// A seeded stream for the shops' cards (splitmix64)
struct Stream(u64);

impl Stream {
    fn new(seed: u64) -> Stream {
        Stream(seed ^ 0x9e37_79b9_7f4a_7c15)
    }
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.unit() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

#[derive(Clone)]
struct Held {
    key: String,
    sell: i64,
    eternal: bool,
}

struct Judge<'a> {
    run: &'a save::RunState,
    data: &'a GameData,
    sims: usize,
    /// the chances the policy picks by, and the ones a branch's boards are scored on (another
    /// seed, more rounds: a pick's own noise isn't counted as its gain)
    memo: Mutex<HashMap<Vec<String>, (f64, f64)>>,
    scored: Mutex<HashMap<Vec<String>, (f64, f64)>>,
    /// the shop pool by rarity (1 Common, 2 Uncommon, 3 Rare): key, price
    pool: [Vec<(String, i64)>; 4],
    boss: String,
}

impl Judge<'_> {
    /// (this ante's boss, the next ante's plain boss) on a board of these jokers, as the
    /// policy sees them
    fn chances(&self, js: &[Held]) -> (f64, f64) {
        self.on(&self.memo, js, self.sims, 42)
    }

    /// The same, on rounds of their own: what a branch's board is scored on
    fn score(&self, js: &[Held]) -> (f64, f64) {
        self.on(&self.scored, js, SCORE_SIMS, 0x5c0e)
    }

    fn on(&self, memo: &Mutex<HashMap<Vec<String>, (f64, f64)>>, js: &[Held], sims: usize, seed: u64) -> (f64, f64) {
        let keys: Vec<String> = js.iter().map(|h| h.key.clone()).collect();
        if let Some(v) = memo.lock().unwrap().get(&keys) {
            return *v;
        }
        let plan = whatif::Plan { jokers: Some(keys.clone()), ..Default::default() };
        let o = whatif::run(self.run, self.data, &plan, sims, seed).expect("whatif");
        let v = (o.plan.iter().find(|b| b.label == self.boss).map_or(1.0, |b| b.p_win), o.plan.last().map_or(0.0, |b| b.p_win));
        memo.lock().unwrap().insert(keys, v);
        v
    }

    /// What the policy plays for: both bosses before this ante's, then the next ante's
    fn aim(&self, js: &[Held], boss_done: bool) -> f64 {
        let (b, n) = self.chances(js);
        if boss_done { n } else { b * n }
    }

    /// The board with `c` added (in a free slot, else for the joker whose sale leaves the
    /// most), the money that sale gives, and what it aims at
    fn with(&self, js: &[Held], c: &Held, boss_done: bool) -> (Vec<Held>, i64, f64) {
        if (js.len() as i64) < self.run.joker_slots {
            let mut v = js.to_vec();
            v.push(c.clone());
            let a = self.aim(&v, boss_done);
            return (v, 0, a);
        }
        let mut best: Option<(Vec<Held>, i64, f64)> = None;
        for i in (0..js.len()).filter(|&i| !js[i].eternal) {
            let mut v = js.to_vec();
            let sold = v.remove(i).sell;
            v.push(c.clone());
            let a = self.aim(&v, boss_done);
            if best.as_ref().is_none_or(|b| a > b.2) {
                best = Some((v, sold, a));
            }
        }
        best.unwrap_or_else(|| (js.to_vec(), 0, self.aim(js, boss_done)))
    }

    /// One shop card: a joker with the shop's joker share, by rarity 70/25/5 as the advice's
    /// draws (`expected_buys`), none you hold: drawn from the whole pool on `s`, a held one
    /// drawn again on `side`, so `s` moves the same in both branches and their other cards stay
    /// the same
    fn draw(&self, s: &mut Stream, side: &mut Stream, share: f64, held: &[Held]) -> Option<Held> {
        if s.unit() >= share {
            return None;
        }
        let roll = s.unit();
        let list = &self.pool[if roll > 0.95 { 3 } else if roll > 0.7 { 2 } else { 1 }];
        if list.is_empty() {
            return None;
        }
        let mut pick = s.below(list.len());
        for _ in 0..32 {
            if !held.iter().any(|h| h.key == list[pick].0) {
                let (k, c) = &list[pick];
                return Some(Held { key: k.clone(), sell: (c / 2).max(1), eternal: false });
            }
            pick = side.below(list.len());
        }
        None
    }

    fn price(&self, key: &str) -> f64 {
        self.data.center(key).map_or(5, |c| c.cost) as f64
    }
}

#[derive(Clone, Copy)]
struct Policy {
    name: &'static str,
    /// money kept back: buys below it only for `big` or more, rerolls never below it
    reserve: f64,
    big: f64,
    rerolls: usize,
}

struct Branch {
    js: Vec<Held>,
    money: f64,
    boss: f64,
    next: f64,
}

fn judge(path: &str, option: &str, trials: usize, sims: usize) -> String {
    let data = GameData::bundled();
    let run = save::load(std::path::Path::new(path), data).expect("load");
    let mut pool: [Vec<(String, i64)>; 4] = Default::default();
    for k in advise::shop_pool(&run, data) {
        if let Some(c) = data.center(&k) {
            pool[c.rarity.unwrap_or(1).min(3) as usize].push((k.clone(), c.cost));
        }
    }
    let Some(boss) = run.blinds.iter().find(|b| b.slot == "Boss") else { return format!("{path}: no boss") };
    let j = Judge { run: &run, data, sims, memo: Mutex::new(HashMap::new()), scored: Mutex::new(HashMap::new()), pool, boss: boss.name.clone() };
    let held0: Vec<Held> = run.jokers.iter().map(|sj| Held { key: sj.key.clone(), sell: sj.sell_value, eternal: sj.eternal }).collect();
    let share = run.shop_rates.joker_share();
    let slots = run.shop_rates.slots.max(1) as usize;
    // the blinds still to play this ante before its boss; the next ante's rewards as this one's
    let ahead: Vec<i64> = run.blinds.iter().filter(|b| b.slot != "Boss" && matches!(b.state.as_str(), "Select" | "Upcoming")).map(|b| b.reward).collect();
    let next_ante: Vec<i64> = run.blinds.iter().filter(|b| b.slot != "Boss").map(|b| b.reward).collect();
    let reroll_now = run.shop.as_ref().map_or(run.base_reroll_cost, |s| s.reroll_cost) as f64;
    let cash = |b: &mut Branch, reward: i64| b.money += reward as f64 + run.money_per_hand + advise::interest(b.money, run.interest_amount, run.interest_cap) as f64;
    let shop = |b: &mut Branch, pol: Policy, boss_done: bool, seed: u64| {
        let mut s = Stream::new(seed);
        let mut side = Stream::new(seed ^ SIDE);
        let mut cards: Vec<Held> = (0..slots).filter_map(|_| j.draw(&mut s, &mut side, share, &b.js)).collect();
        let mut rerolls = 0;
        loop {
            let now = j.aim(&b.js, boss_done);
            let mut best: Option<(usize, Vec<Held>, i64, f64)> = None;
            for (i, c) in cards.iter().enumerate().filter(|(_, c)| j.price(&c.key) <= b.money) {
                let (v, sold, a) = j.with(&b.js, c, boss_done);
                let gain = a - now;
                if gain > 0.005 && (b.money - j.price(&c.key) >= pol.reserve || gain >= pol.big) && best.as_ref().is_none_or(|x| a > x.3) {
                    best = Some((i, v, sold, a));
                }
            }
            if let Some((i, v, sold, _)) = best {
                let c = cards.remove(i);
                b.money += sold as f64 - j.price(&c.key);
                b.js = v;
                continue;
            }
            let cost = (run.base_reroll_cost + rerolls as i64) as f64;
            if rerolls < pol.rerolls && b.money - cost >= pol.reserve {
                b.money -= cost;
                rerolls += 1;
                let mut s = Stream::new(seed ^ (rerolls as u64).wrapping_mul(0x1234_5677));
                let mut side = Stream::new(seed ^ (rerolls as u64).wrapping_mul(0x1234_5677) ^ SIDE);
                cards = (0..slots).filter_map(|_| j.draw(&mut s, &mut side, share, &b.js)).collect();
                continue;
            }
            break;
        }
    };
    let play = |pol: Policy, take: bool, t: usize| -> Branch {
        let mut b = Branch { js: held0.clone(), money: run.dollars, boss: 0.0, next: 0.0 };
        if take {
            if let Some(k) = option.strip_prefix("joker:") {
                let cost = run.shop.as_ref().and_then(|s| s.jokers.iter().find(|x| x.key == k)).map_or_else(|| j.price(k), |x| x.cost as f64);
                let (v, sold, _) = j.with(&b.js, &Held { key: k.to_string(), sell: ((cost as i64) / 2).max(1), eternal: false }, false);
                b.js = v;
                b.money += sold as f64 - cost;
            } else {
                // a reroll's cards are bought, a Buffoon pack's picked free
                let (n, sh, cost, pay) = match option.strip_prefix("pack:").and_then(|r| r.split_once(':')) {
                    Some((n, c)) => (n.parse().unwrap_or(2), 1.0, c.parse().unwrap_or(4.0), false),
                    None => (slots, share, reroll_now, true),
                };
                b.money -= cost;
                let mut s = Stream::new(0xabc ^ (t as u64).wrapping_mul(977));
                let mut side = Stream::new(0xabc ^ (t as u64).wrapping_mul(977) ^ SIDE);
                let cards: Vec<Held> = (0..n).filter_map(|_| j.draw(&mut s, &mut side, sh, &b.js)).collect();
                let now = j.aim(&b.js, false);
                let mut best: Option<(Vec<Held>, i64, f64, f64)> = None;
                for c in &cards {
                    let price = if pay { j.price(&c.key) } else { 0.0 };
                    if price > b.money {
                        continue;
                    }
                    let (v, sold, a) = j.with(&b.js, c, false);
                    if a > now + 0.005 && best.as_ref().is_none_or(|x| a > x.2) {
                        best = Some((v, sold, a, price));
                    }
                }
                if let Some((v, sold, _, price)) = best {
                    b.js = v;
                    b.money += sold as f64 - price;
                }
            }
        }
        let mut k = 0u64;
        let mut next_shop = |b: &mut Branch, reward: i64, boss_done: bool| {
            cash(b, reward);
            k += 1;
            shop(b, pol, boss_done, (t as u64) << 20 | k);
        };
        for &r in &ahead {
            next_shop(&mut b, r, false);
        }
        b.boss = j.score(&b.js).0;
        next_shop(&mut b, boss.reward, true);
        for &r in &next_ante {
            next_shop(&mut b, r, true);
        }
        b.next = j.score(&b.js).1;
        b
    };
    let (b0, n0) = j.score(&held0);
    let mut out = format!("{path} {option}: ante {} ${} · now {} {b0:.3}, next ante {n0:.3}\n", run.ante, run.dollars, boss.name);
    let policies = [
        Policy { name: "spend", reserve: 0.0, big: 0.0, rerolls: 3 },
        Policy { name: "cap", reserve: run.interest_cap as f64, big: 0.05, rerolls: 3 },
    ];
    for pol in policies {
        let res: Vec<(Branch, Branch)> = (0..trials).map(|t| (play(pol, true, t), play(pol, false, t))).collect();
        let stat = |f: &dyn Fn(&Branch, &Branch) -> f64| {
            let v: Vec<f64> = res.iter().map(|(a, b)| f(a, b)).collect();
            let m = v.iter().sum::<f64>() / v.len() as f64;
            let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len().max(2) - 1) as f64).sqrt();
            (m, sd / (v.len() as f64).sqrt())
        };
        let (d, se) = stat(&|a, b| a.boss * a.next - b.boss * b.next);
        let (db, seb) = stat(&|a, b| a.boss - b.boss);
        let (dn, sen) = stat(&|a, b| a.next - b.next);
        let (ma, _) = stat(&|a, _| a.money);
        let (dm, _) = stat(&|a, b| a.money - b.money);
        out += &format!(
            "  {:5}: both bosses {d:+.3} ± {se:.3} against leaving (this boss {db:+.3} ± {seb:.3}, next {dn:+.3} ± {sen:.3}); money at the next boss ${ma:.1} ({dm:+.1})\n",
            pol.name
        );
    }
    out
}

#[test]
#[ignore]
fn shop_playon() {
    let Some(list) = std::env::var_os("BAV_PLAYON") else { panic!("set BAV_PLAYON to a list of `<state.jkr> <option>` lines") };
    let num = |k: &str, d: usize| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
    let (trials, sims) = (num("BAV_PLAYON_TRIALS", 24), num("BAV_PLAYON_SIMS", 150));
    for line in std::fs::read_to_string(list).unwrap().lines().filter(|l| !l.trim().is_empty()) {
        let (path, option) = line.trim().split_once(' ').expect("`<state.jkr> <option>`");
        print!("{}", judge(path, option.trim(), trials, sims));
    }
}
