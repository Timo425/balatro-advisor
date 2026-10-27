//! A shop option played on against leaving the shop, for judging what the shop ranking says
//! about spending (gap 17 in docs/design.md, measured with it 2026-10-24). From a saved shop
//! state: the option taken (a joker from the shop, a reroll, a Buffoon pack) or not, then the
//! shops up to the next ante's boss played by one fixed policy in both branches, the same
//! shop cards for both (paired), each board's chances at this ante's boss and the next ante's
//! plain boss simulated by `whatif` (the engine's rounds), money with the game's interest and
//! the blinds' rewards. It shares the engine with the advice, not its valuation: the jokers are
//! held, not valued; each board picked on one set of rounds and scored on another, 400
//! rounds, so a pick's own noise isn't counted as its gain. The policy is a heuristic: in each shop, buy the joker that most raises
//! the boss's chance ahead (this ante's × the next ante's, at least 3 wins in its rounds, before this boss), in a free slot or
//! for the joker whose sale leaves the most, rerolling up to 3 times; "spend" buys and rerolls
//! while it can pay, "cap" keeps the interest cap (buys below it only for 5 points or more,
//! rerolls only above it). Not counted: editions, packs, planets and tarots in later shops,
//! vouchers, skips, rent, the hands' money beyond $1 a round, anything after the next ante's
//! boss (the money left there is printed). Slow (a board with full slots tries every swap):
//! `BAV_PLAYON=list.txt cargo test --release --test shop_playon -- --ignored --nocapture`, a
//! list of lines `<state.jkr> <option>`: `joker:<key>`, `reroll`, `pack:<jokers>:<price>[:<picks>]`
//! (a Buffoon pack), `celestial:<planets>:<price>` (a Celestial pack, one picked: the hand it
//! levels counted for the rest of the branch), `planet:<hand>:<price>`, `sell:<key>`, `leave`
//! (the money you hold played on through the shops ahead, from your board's chances), `money`
//! (the same with this shop's rerolls first); both also with "spend8", rerolling up to 8 times a
//! shop as the advice's money walk may; `BAV_PLAYON_TRIALS` (24), `BAV_PLAYON_SIMS` (150).

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

/// Chances memoised by (the jokers, the hands levelled up)
type Memo = Mutex<HashMap<(Vec<String>, Vec<String>), (f64, f64)>>;

struct Judge<'a> {
    run: &'a save::RunState,
    data: &'a GameData,
    sims: usize,
    /// the chances the policy picks by, and the ones a branch's boards are scored on (another
    /// seed, more rounds: a pick's own noise isn't counted as its gain)
    memo: Memo,
    scored: Memo,
    /// the shop pool by rarity (1 Common, 2 Uncommon, 3 Rare): key, price
    pool: [Vec<(String, i64)>; 4],
    boss: String,
}

impl Judge<'_> {
    /// (this ante's boss, the next ante's plain boss) on a board of these jokers, as the
    /// policy sees them
    fn chances(&self, js: &[Held], lv: &[String]) -> (f64, f64) {
        self.on(&self.memo, js, lv, self.sims, 42)
    }

    /// The same, on rounds of their own: what a branch's board is scored on
    fn score(&self, js: &[Held], lv: &[String]) -> (f64, f64) {
        self.on(&self.scored, js, lv, SCORE_SIMS, 0x5c0e)
    }

    /// `lv`: the hands a planet levelled up in this branch (once each time named)
    fn on(&self, memo: &Memo, js: &[Held], lv: &[String], sims: usize, seed: u64) -> (f64, f64) {
        let keys: Vec<String> = js.iter().map(|h| h.key.clone()).collect();
        let mut levels = lv.to_vec();
        levels.sort();
        let key = (keys, levels);
        if let Some(v) = memo.lock().unwrap().get(&key) {
            return *v;
        }
        let mut run = self.run.clone();
        for h in lv {
            if let Some(l) = run.hand_levels.get_mut(h) {
                l.level += 1;
                l.chips += l.l_chips;
                l.mult += l.l_mult;
            }
        }
        let plan = whatif::Plan { jokers: Some(key.0.clone()), ..Default::default() };
        let o = whatif::run(&run, self.data, &plan, sims, seed).expect("whatif");
        let v = (o.plan.iter().find(|b| b.label == self.boss).map_or(1.0, |b| b.p_win), o.plan.last().map_or(0.0, |b| b.p_win));
        memo.lock().unwrap().insert(key, v);
        v
    }

    /// What the policy plays for: both bosses before this ante's, then the next ante's
    /// (a next ante hopeless on every board, under 3 wins in the policy's rounds, counts that
    /// much, as the shop ranking's floor does: else no joker would be bought for this boss)
    fn aim(&self, js: &[Held], lv: &[String], boss_done: bool) -> f64 {
        let (b, n) = self.chances(js, lv);
        if boss_done { n } else { b * n.max(3.0 / self.sims as f64) }
    }

    /// The board with `c` added (in a free slot, else for the joker whose sale leaves the
    /// most), the money that sale gives, and what it aims at
    fn with(&self, js: &[Held], lv: &[String], c: &Held, boss_done: bool) -> (Vec<Held>, i64, f64) {
        if (js.len() as i64) < self.run.joker_slots {
            let mut v = js.to_vec();
            v.push(c.clone());
            let a = self.aim(&v, lv, boss_done);
            return (v, 0, a);
        }
        let mut best: Option<(Vec<Held>, i64, f64)> = None;
        for i in (0..js.len()).filter(|&i| !js[i].eternal) {
            let mut v = js.to_vec();
            let sold = v.remove(i).sell;
            v.push(c.clone());
            let a = self.aim(&v, lv, boss_done);
            if best.as_ref().is_none_or(|b| a > b.2) {
                best = Some((v, sold, a));
            }
        }
        best.unwrap_or_else(|| (js.to_vec(), 0, self.aim(js, lv, boss_done)))
    }

    /// One shop card: a joker with the shop's joker share, by rarity 70/25/5 as the advice's
    /// walk (`money_value_with`), none you hold: drawn from the whole pool on `s`, a held one
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
    /// the hands a planet levelled up
    lv: Vec<String>,
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
    // free rerolls: one a shop for each Chaos the Clown (state_events.lua `new_round`); in the shop
    // you're in, the ones the save says are left
    let chaos = run.jokers.iter().filter(|j| j.key == "j_chaos").count();
    let free_now = run.shop.as_ref().map_or(0, |s| s.free_rerolls.max(0) as usize);
    // the hands a Celestial pack's planets level: every planet's, a secret hand's once played
    let planets: Vec<String> = data
        .centers
        .iter()
        .filter(|c| c.set == "Planet")
        .filter_map(|c| c.config.get("hand_type")?.as_str().map(str::to_string))
        .filter(|h| run.hand_levels.get(h).is_some_and(|l| l.played > 0 || !matches!(h.as_str(), "Five of a Kind" | "Flush House" | "Flush Five")))
        .collect();
    let cash = |b: &mut Branch, reward: i64| b.money += reward as f64 + run.money_per_hand + advise::interest(b.money, run.interest_amount, run.interest_cap) as f64;
    // `here`: the shop you're in, its cards left (they're the options), rerolls from its price
    let shop = |b: &mut Branch, pol: Policy, boss_done: bool, seed: u64, here: bool| {
        let mut s = Stream::new(seed);
        let mut side = Stream::new(seed ^ SIDE);
        let mut cards: Vec<Held> = if here { vec![] } else { (0..slots).filter_map(|_| j.draw(&mut s, &mut side, share, &b.js)).collect() };
        let mut rerolls = 0;
        loop {
            let now = j.aim(&b.js, &b.lv, boss_done);
            let mut best: Option<(usize, Vec<Held>, i64, f64)> = None;
            for (i, c) in cards.iter().enumerate().filter(|(_, c)| j.price(&c.key) <= b.money) {
                let (v, sold, a) = j.with(&b.js, &b.lv, c, boss_done);
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
            let free = if here { free_now } else { chaos };
            let start = if here && free_now == 0 { reroll_now } else { run.base_reroll_cost as f64 };
            let cost = if rerolls < free { 0.0 } else { start + (rerolls - free) as f64 };
            if rerolls < pol.rerolls + free && b.money - cost >= pol.reserve {
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
        let mut b = Branch { js: held0.clone(), lv: vec![], money: run.dollars, boss: 0.0, next: 0.0 };
        if take {
            if let Some(k) = option.strip_prefix("joker:") {
                let cost = run.shop.as_ref().and_then(|s| s.jokers.iter().find(|x| x.key == k)).map_or_else(|| j.price(k), |x| x.cost as f64);
                let (v, sold, _) = j.with(&b.js, &b.lv, &Held { key: k.to_string(), sell: ((cost as i64) / 2).max(1), eternal: false }, false);
                b.js = v;
                b.money += sold as f64 - cost;
            } else if let Some(k) = option.strip_prefix("sell:") {
                if let Some(i) = b.js.iter().position(|h| h.key == k) {
                    b.money += b.js.remove(i).sell as f64;
                }
            } else if let Some((hand, cost)) = option.strip_prefix("planet:").and_then(|r| r.rsplit_once(':')) {
                b.money -= cost.parse::<f64>().unwrap_or(3.0);
                b.lv.push(hand.to_string());
            } else if let Some((n, cost)) = option.strip_prefix("celestial:").and_then(|r| r.split_once(':')) {
                // a Celestial pack: `n` planets of the hands you can level, one picked
                b.money -= cost.parse::<f64>().unwrap_or(4.0);
                let mut s = Stream::new(0xce1 ^ (t as u64).wrapping_mul(977));
                let mut hands = planets.clone();
                let mut shown = vec![];
                for _ in 0..n.parse().unwrap_or(3usize).min(hands.len()) {
                    shown.push(hands.remove(s.below(hands.len())));
                }
                let now = j.aim(&b.js, &b.lv, false);
                let pick = shown.into_iter().map(|h| {
                    let mut lv = b.lv.clone();
                    lv.push(h);
                    (j.aim(&b.js, &lv, false), lv)
                }).max_by(|x, y| x.0.total_cmp(&y.0));
                if let Some((_, lv)) = pick.filter(|p| p.0 > now) {
                    b.lv = lv;
                }
            } else {
                assert!(option == "reroll" || option.starts_with("pack:"), "unknown option {option}");
                // a reroll's cards are bought, a Buffoon pack's picked free (a Mega pack: two picks)
                let parts: Vec<&str> = option.strip_prefix("pack:").map(|r| r.split(':').collect()).unwrap_or_default();
                let (n, sh, cost, pay, picks) = match parts.as_slice() {
                    [n, c, rest @ ..] => (n.parse().unwrap_or(2), 1.0, c.parse().unwrap_or(4.0), false, rest.first().and_then(|p| p.parse().ok()).unwrap_or(1)),
                    _ => (slots, share, reroll_now, true, 1),
                };
                b.money -= cost;
                let mut s = Stream::new(0xabc ^ (t as u64).wrapping_mul(977));
                let mut side = Stream::new(0xabc ^ (t as u64).wrapping_mul(977) ^ SIDE);
                let mut cards: Vec<Held> = (0..n).filter_map(|_| j.draw(&mut s, &mut side, sh, &b.js)).collect();
                for _ in 0..picks {
                    let now = j.aim(&b.js, &b.lv, false);
                    let mut best: Option<(usize, Vec<Held>, i64, f64, f64)> = None;
                    for (i, c) in cards.iter().enumerate() {
                        let price = if pay { j.price(&c.key) } else { 0.0 };
                        if price > b.money {
                            continue;
                        }
                        let (v, sold, a) = j.with(&b.js, &b.lv, c, false);
                        if a > now + 0.005 && best.as_ref().is_none_or(|x| a > x.3) {
                            best = Some((i, v, sold, a, price));
                        }
                    }
                    let Some((i, v, sold, _, price)) = best else { break };
                    cards.remove(i);
                    b.js = v;
                    b.money += sold as f64 - price;
                }
            }
        }
        let mut k = 0u64;
        let mut next_shop = |b: &mut Branch, reward: i64, boss_done: bool| {
            cash(b, reward);
            k += 1;
            shop(b, pol, boss_done, (t as u64) << 20 | k, false);
        };
        if option == "money" {
            // this shop's rerolls first, as the advice's money buys them
            shop(&mut b, pol, false, (t as u64) << 20 | 0xffff, true);
        }
        for &r in &ahead {
            next_shop(&mut b, r, false);
        }
        b.boss = j.score(&b.js, &b.lv).0;
        next_shop(&mut b, boss.reward, true);
        for &r in &next_ante {
            next_shop(&mut b, r, true);
        }
        b.next = j.score(&b.js, &b.lv).1;
        b
    };
    let (b0, n0) = j.score(&held0, &[]);
    let mut out = format!("{path} {option}: ante {} ${} · now {} {b0:.3}, next ante {n0:.3}\n", run.ante, run.dollars, boss.name);
    let policies = [
        Policy { name: "spend", reserve: 0.0, big: 0.0, rerolls: 3 },
        Policy { name: "cap", reserve: run.interest_cap as f64, big: 0.05, rerolls: 3 },
    ];
    // the money's own play-on also rerolls up to 8 times a shop, as the advice's walk may
    let more = [Policy { name: "spend8", reserve: 0.0, big: 0.0, rerolls: 8 }];
    let policies: Vec<Policy> = policies.into_iter().chain(if option == "money" || option == "leave" { more.to_vec() } else { vec![] }).collect();
    for pol in policies {
        if option == "leave" || option == "money" {
            // leaving alone: what the shops ahead add with the money you hold, played on
            let res: Vec<Branch> = (0..trials).map(|t| play(pol, false, t)).collect();
            let (db, seb) = mean_se(&res.iter().map(|b| b.boss - b0).collect::<Vec<_>>());
            let (dn, sen) = mean_se(&res.iter().map(|b| b.next - n0).collect::<Vec<_>>());
            let (m, _) = mean_se(&res.iter().map(|b| b.money).collect::<Vec<_>>());
            out += &format!("  {:5}: this boss {db:+.3} ± {seb:.3}, next {dn:+.3} ± {sen:.3} from the shops ahead; money at the next boss ${m:.1}\n", pol.name);
            continue;
        }
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

/// The mean of `v` and its standard error
fn mean_se(v: &[f64]) -> (f64, f64) {
    let m = v.iter().sum::<f64>() / v.len().max(1) as f64;
    let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len().max(2) - 1) as f64).sqrt();
    (m, sd / (v.len().max(1) as f64).sqrt())
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

/// What the shop ranking's money's finds are made of, on one shop state (gap 17): the
/// advice's lift from the money you hold (leaving's survival less its chance, this ante's
/// hardest round and the next ante's boss), and the joker pool's chances as screened against
/// the same boards simulated again on rounds of their own (`FRESH` rounds, another seed), each
/// a gain over your board's on the same rounds; then the same money played on as leaving
/// spends it (`judge`'s "leave": the shops ahead, not this one's rerolls, as the advice's
/// "next round" walks out of the shop, `Stay::Out`).
const FRESH: usize = 400;

fn money_finds(path: &str, trials: usize, sims: usize) -> String {
    let data = GameData::bundled();
    let run = save::load(std::path::Path::new(path), data).expect("load");
    let a = advise::analyze(&run, data, None, &advise::Options::default());
    let key = a.options_round;
    let Some(hz) = a.rounds.iter().position(|r| r.horizon) else { return format!("{path}: no next ante\n") };
    let Some(leave) = a.options.iter().find(|o| o.kind == "leave") else { return format!("{path}: no leaving\n") };
    let (p_now, p_next) = (a.rounds[key].p_win, a.rounds[hz].p_win);
    let lift = (leave.survive.unwrap_or(p_now) - p_now, leave.survive_next.unwrap_or(p_next) - p_next);
    let mut out = format!(
        "{path}: ante {} ${} {}/{} jokers · {} {p_now:.3}, next ante {p_next:.3} · advice's lift from the money: this boss {:+.3}, next {:+.3}\n",
        run.ante, run.dollars, run.jokers.len(), run.joker_slots, a.rounds[key].blind, lift.0, lift.1
    );
    if std::env::var_os("BAV_FINDS_QUICK").is_some() {
        out += &judge(path, "leave", trials, sims).lines().skip(1).map(|l| format!("  played on {}\n", l.trim())).collect::<String>();
        return out;
    }
    // the pool simulated again: each board as the screen built it
    let boss = a.rounds[key].blind.clone();
    let owned: Vec<String> = run.jokers.iter().map(|j| j.key.clone()).collect();
    let fresh = |keys: Vec<String>| {
        let plan = whatif::Plan { jokers: Some(keys), ..Default::default() };
        let o = whatif::run(&run, data, &plan, FRESH, 0xf2e5).expect("whatif");
        (o.plan.iter().find(|b| b.label == boss).map_or(1.0, |b| b.p_win), o.plan.last().map_or(0.0, |b| b.p_win))
    };
    let (fb, fnx) = fresh(owned.clone());
    let pool = &a.pool;
    let again: Vec<(f64, f64)> = pool
        .iter()
        .map(|c| {
            let mut keys = owned.clone();
            match c.sell_index {
                Some(i) if c.action.contains("rightmost") => {
                    keys.remove(i);
                    keys.push(c.key.clone());
                }
                Some(i) => keys[i] = c.key.clone(),
                None if c.action.contains("leftmost") => keys.insert(0, c.key.clone()),
                None => keys.push(c.key.clone()),
            }
            fresh(keys)
        })
        .collect();
    // the screen's gains as the advice reads them (`measured_p`: at least 3 wins), the fresh ones
    let n_key = pool.first().map_or(60, |c| c.sims);
    let n_hz = (n_key / 3).max(20);
    let screened = |i: usize, r: usize, p0: f64, n: usize| pool[i].p_win.get(r).copied().filter(|&p| p >= 3.0 / n as f64).map_or(0.0, |p| (p - p0).max(0.0));
    let g_key: Vec<f64> = (0..pool.len()).map(|i| screened(i, key, p_now, if pool[i].precise { 300 } else { n_key })).collect();
    let g_hz: Vec<f64> = (0..pool.len()).map(|i| screened(i, hz, p_next, if pool[i].precise { 100 } else { n_hz })).collect();
    let f_key: Vec<f64> = again.iter().map(|x| (x.0 - fb).max(0.0)).collect();
    let f_hz: Vec<f64> = again.iter().map(|x| (x.1 - fnx).max(0.0)).collect();
    let top = |g: &[f64], f: &[f64], k: usize| {
        let mut ix: Vec<usize> = (0..g.len()).collect();
        ix.sort_by(|&x, &y| g[y].total_cmp(&g[x]));
        let ix: Vec<usize> = ix.into_iter().take(k).collect();
        (ix.iter().map(|&i| g[i]).sum::<f64>() / k as f64, ix.iter().map(|&i| f[i]).sum::<f64>() / k as f64)
    };
    let ((s12, f12), (sn12, fn12)) = (top(&g_key, &f_key, 12), top(&g_hz, &f_hz, 12));
    out += &format!(
        "  your board again: this boss {fb:.3}, next {fnx:.3} · the 12 best screened gains: this boss {s12:+.3} → {f12:+.3} again, next {sn12:+.3} → {fn12:+.3} again\n"
    );
    out += &judge(path, "leave", trials, sims).lines().skip(1).map(|l| format!("  played on {}\n", l.trim())).collect::<String>();
    out
}

/// `BAV_FINDS=list.txt cargo test --release --test shop_playon -- --ignored --nocapture
/// money_finds`: a list of shop states, one per line (`BAV_FINDS_QUICK`: the lifts and the
/// play-on only, not the pool simulated again); at the end, the advice's lift less each policy's
/// played on, on average and on average apart from its sign (gap 17's measure reads "spend8",
/// whose rerolls are the walk's budget; its states are
/// `tests/fixtures/private/money_finds_states.txt`)
#[test]
#[ignore]
fn money_finds_test() {
    let Some(list) = std::env::var_os("BAV_FINDS") else { panic!("set BAV_FINDS to a list of shop states") };
    let num = |k: &str, d: usize| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
    let (trials, sims) = (num("BAV_PLAYON_TRIALS", 24), num("BAV_PLAYON_SIMS", 150));
    // the first number after `key` in `line`
    let after = |line: &str, key: &str| line.split(key).nth(1).and_then(|r| r.split([',', ' ']).find(|t| !t.is_empty())).and_then(|t| t.parse::<f64>().ok());
    let mut errors: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    for line in std::fs::read_to_string(list).unwrap().lines().filter(|l| !l.trim().is_empty()) {
        let out = money_finds(line.trim(), trials, sims);
        print!("{out}");
        let advice = out.lines().next().and_then(|l| l.split("lift from the money:").nth(1)).and_then(|l| Some((after(l, "this boss ")?, after(l, "next ")?)));
        // (each policy's line: "  played on <name>: this boss …, next … from the shops ahead")
        for l in out.lines().filter(|l| l.contains("from the shops ahead")) {
            let name = l.trim().trim_start_matches("played on ").split(':').next().unwrap_or("").trim().to_string();
            if let (Some(a), Some(b), Some(n)) = (advice, after(l, "this boss "), after(l, "next ")) {
                errors.entry(name).or_default().push((a.0 - b, a.1 - n));
            }
        }
    }
    let mut names: Vec<&String> = errors.keys().collect();
    names.sort();
    for name in names {
        let e = &errors[name];
        let mean = |f: &dyn Fn(&(f64, f64)) -> f64| e.iter().map(f).sum::<f64>() / e.len().max(1) as f64;
        println!(
            "{name}, {} states: the advice less played on, this boss {:+.3} (apart from sign {:.3}), next ante's boss {:+.3} ({:.3})",
            e.len(),
            mean(&|e| e.0),
            mean(&|e| e.0.abs()),
            mean(&|e| e.1),
            mean(&|e| e.1.abs())
        );
    }
}
