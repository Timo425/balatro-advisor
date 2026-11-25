//! A shop option played on against leaving the shop, for judging what the shop ranking says
//! about spending (gap 17 in docs/design.md, measured with it 2026-10-24). From a saved shop
//! state: the option taken (a joker from the shop, a reroll, a pack, a tarot) or not, then the
//! shops up to the next ante's boss played by one fixed policy in both branches, the same
//! shop cards for both and every branch's rounds dealt from its deck in one fixed order, the
//! advice's sorted deck (paired; since 2026-11-19, before it the save's draw pile), each board's chances at this ante's boss and the next ante's
//! plain boss simulated by `whatif` (the engine's rounds), money with the game's interest and
//! the blinds' rewards. It shares the engine with the advice, not its valuation: the jokers are
//! held, not valued; each board picked on one set of rounds and scored on another, 400
//! rounds, so a pick's own noise isn't counted as its gain. The policy is a heuristic: in each shop, buy the joker that most raises
//! the boss's chance ahead (this ante's × the next ante's, at least 3 wins in its rounds, before this boss), in a free slot or
//! for the joker whose sale leaves the most, rerolling up to 3 times; "spend" buys and rerolls
//! while it can pay, "cap" keeps the interest cap (buys below it only for 5 points or more,
//! rerolls only above it). Not counted: editions, packs, planets and tarots in later shops,
//! vouchers but Seed Money's and Money Tree's cap, skips, rent, the hands' money beyond $1 a
//! round, anything after the next ante's boss (the money left there is printed).
//!
//! A tarot or spectral (bought, picked from a pack, made by The Emperor or copied by The Fool) is
//! used at once, as the advice uses it (`advise::TarotValue`, the shop's own analysis): a card
//! effect on the deck the advice's target race left (its sampled hands' decks, one per trial:
//! the hand you'd hold it in, not any card in the deck), Sigil, Ouija, Immolate, Familiar, Grim
//! and Incantation on the decks it valued them by, Ouija's hand size; the rest by the game's own
//! effect (card.lua `Card:use_consumeable`, `Card:can_use_consumeable`): The Hermit's and
//! Temperance's money, Judgement's, Wraith's and The Soul's joker (a full board sells the joker
//! whose sale leaves the most, when that beats not using it), The Wheel of Fortune's, Hex's and
//! Ectoplasm's edition on a random joker without one (Hex destroying the rest, Ectoplasm −1 hand
//! size), Ankh's copy, Black Hole's levels, The High Priestess's 2 random planets, The Emperor's
//! 2 random tarots (each used when the advice values it above ×1.00, else sold for $1) up to the
//! consumable slots you have free. An Arcana or Spectral pack (`arcana:` / `spectral:`) shows
//! its cards drawn from the game's pool (not one you hold, Omen Globe's spectral in an Arcana
//! pack, The Soul and Black Hole at the game's 0.3%), and its pick is the advice's (the card it
//! values most by Ante 8, `TarotValue::long_mult`, none below ×1.00: the skip; the advice's own
//! skip is `SkipPack`'s worth, above 1 with Red Card, and it counts no Mega pack's second pick,
//! which this plays), used as above.
//! A use the advice doesn't simulate (Medium's Purple Seal, a tarot it gives no deck) can't be
//! played: the option says so and isn't judged. Not counted: a held tarot saved for a better
//! moment (used at once here), a second deck change on a first that destroyed or added cards
//! (counted as not played), a tarot's sell value but The Emperor's.
//!
//! Slow (a board with full slots tries every swap):
//! `BAV_PLAYON=list.txt cargo test --release --test shop_playon -- --ignored --nocapture`, a
//! list of lines `<state.jkr> <option>`: `joker:<key>[:<edition>]`, `reroll`, `pack:<jokers>:<price>[:<picks>]`
//! (a Buffoon pack), `celestial:<planets>:<price>[:<picks>]` (a Celestial pack, the best one picked, a Mega
//! pack's two: the hands they level counted for the rest of the branch), `planet:<hand>:<price>`, `tarot:<key>[:<price>]`
//! (a tarot or spectral from the shop, used at once), `arcana:<tarots>:<price>[:<picks>]`,
//! `spectral:<cards>:<price>[:<picks>]`, `voucher:<key>:<price>` (Seed Money, Money Tree),
//! `sell:<key>[:<place in your row, from 1>]`, `leave` (the money you hold played on through the shops ahead, from your
//! board's chances), `money` (the same with this shop's rerolls first); both also with
//! "spend8", rerolling up to 8 times a shop as the advice's money walk may;
//! `BAV_PLAYON_TRIALS` (24), `BAV_PLAYON_SIMS` (150); `BAV_PLAYON_SHOW`: each deck change a
//! tarot makes, and your board's chances on that deck (to check against `whatif --card`).
//!
//! `shop_judge`: a change's moved top picks judged by this, from two runs of `shop_picks.rs`
//! (before and after the change).

#[path = "shop_picks.rs"]
mod shop_picks;
use shop_picks::stamp;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use balatro_advisor::engine::consumable::GUARANTEED_EDITION;
use balatro_advisor::model::{Card, Edition};
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
    /// its edition, as `whatif` names one ("foil", "holo", "poly", "negative")
    ed: Option<&'static str>,
    sell: i64,
    eternal: bool,
}

impl Held {
    /// the joker as `whatif` names it, with its edition
    fn spec(&self) -> String {
        self.ed.map_or_else(|| self.key.clone(), |e| format!("{}:{e}", self.key))
    }
}

fn ed_name(e: Edition) -> &'static str {
    match e {
        Edition::Foil => "foil",
        Edition::Holo => "holo",
        Edition::Polychrome => "poly",
        Edition::Negative => "negative",
    }
}

/// A branch's board besides its jokers: the hands a planet levelled up (once each time named),
/// the deck a tarot changed (none: yours as it is), the hand size a spectral took away
#[derive(Clone, Default)]
struct Rest {
    lv: Vec<String>,
    deck: Option<Vec<Card>>,
    size: i64,
}

/// Chances memoised by (the jokers, the hands levelled up, the deck changed, the hand size)
type Memo = Mutex<HashMap<(Vec<String>, Vec<String>, u64, i64), (f64, f64)>>;

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
    /// your joker slots without the Negatives you hold (each of them adds one: card.lua
    /// `Card:add_to_deck`)
    base_slots: i64,
    /// how the advice values each tarot and spectral on this shop (`analyze`'s `tarots`), when an
    /// option uses one
    tarots: Vec<advise::TarotValue>,
    /// the deck those values changed (`advise`'s fresh deck: the full deck in its fixed order)
    fresh: Vec<Card>,
    /// the hands a random planet levels (a secret hand's once played)
    planets: Vec<String>,
}

impl Judge<'_> {
    /// (this ante's boss, the next ante's plain boss) on a board of these jokers, as the
    /// policy sees them
    fn chances(&self, js: &[Held], r: &Rest) -> (f64, f64) {
        self.on(&self.memo, js, r, self.sims, 42)
    }

    /// The same, on rounds of their own: what a branch's board is scored on
    fn score(&self, js: &[Held], r: &Rest) -> (f64, f64) {
        self.on(&self.scored, js, r, SCORE_SIMS, 0x5c0e)
    }

    fn on(&self, memo: &Memo, js: &[Held], r: &Rest, sims: usize, seed: u64) -> (f64, f64) {
        let keys: Vec<String> = js.iter().map(Held::spec).collect();
        let mut levels = r.lv.clone();
        levels.sort();
        let deck = r.deck.as_ref().map_or(0, |d| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            format!("{d:?}").hash(&mut h);
            h.finish() | 1
        });
        let key = (keys, levels, deck, r.size);
        if let Some(v) = memo.lock().unwrap().get(&key) {
            return *v;
        }
        let mut run = self.run.clone();
        for h in &r.lv {
            if let Some(l) = run.hand_levels.get_mut(h) {
                l.level += 1;
                l.chips += l.l_chips;
                l.mult += l.l_mult;
            }
        }
        // every branch on its deck in the fixed order, yours unchanged too: the rounds are dealt
        // from the deck's order, so a change of one card keeps the rest of the rounds the same
        // (on the save's order, the 1RVL49LQ a4-r8 shop's next ante read 0.007, on the same deck
        // in the fixed order 0.028: the pairing was lost). In a shop every card is in the deck.
        run.draw_pile = r.deck.as_ref().unwrap_or(&self.fresh).clone();
        run.hand.clear();
        run.discard_pile.clear();
        run.hand_size = (run.hand_size + r.size).max(1);
        let plan = whatif::Plan { jokers: Some(key.0.clone()), ..Default::default() };
        let o = whatif::run(&run, self.data, &plan, sims, seed).expect("whatif");
        let v = (o.plan.iter().find(|b| b.label == self.boss).map_or(1.0, |b| b.p_win), o.plan.last().map_or(0.0, |b| b.p_win));
        memo.lock().unwrap().insert(key, v);
        v
    }

    /// What the policy plays for: both bosses before this ante's, then the next ante's
    /// (a next ante hopeless on every board, under 3 wins in the policy's rounds, counts that
    /// much, as the shop ranking's floor does: else no joker would be bought for this boss)
    fn aim(&self, js: &[Held], r: &Rest, boss_done: bool) -> f64 {
        let (b, n) = self.chances(js, r);
        if boss_done { n } else { b * n.max(3.0 / self.sims as f64) }
    }

    fn slots(&self, js: &[Held]) -> i64 {
        self.base_slots + js.iter().filter(|h| h.ed == Some("negative")).count() as i64
    }

    /// The board with `c` added (in a free slot, else for the joker whose sale leaves the
    /// most), the money that sale gives, and what it aims at
    fn with(&self, js: &[Held], r: &Rest, c: &Held, boss_done: bool) -> (Vec<Held>, i64, f64) {
        if (js.len() as i64) < self.slots(js) || c.ed == Some("negative") {
            let mut v = js.to_vec();
            v.push(c.clone());
            let a = self.aim(&v, r, boss_done);
            return (v, 0, a);
        }
        let mut best: Option<(Vec<Held>, i64, f64)> = None;
        for i in (0..js.len()).filter(|&i| !js[i].eternal) {
            let mut v = js.to_vec();
            let sold = v.remove(i).sell;
            v.push(c.clone());
            let a = self.aim(&v, r, boss_done);
            if best.as_ref().is_none_or(|b| a > b.2) {
                best = Some((v, sold, a));
            }
        }
        best.unwrap_or_else(|| (js.to_vec(), 0, self.aim(js, r, boss_done)))
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
        self.draw_from(&self.pool[if roll > 0.95 { 3 } else if roll > 0.7 { 2 } else { 1 }], s, side, held)
    }

    /// One of `list`, none you hold (a held one drawn again on `side`)
    fn draw_from(&self, list: &[(String, i64)], s: &mut Stream, side: &mut Stream, held: &[Held]) -> Option<Held> {
        if list.is_empty() {
            return None;
        }
        let mut pick = s.below(list.len());
        for _ in 0..32 {
            if !held.iter().any(|h| h.key == list[pick].0) {
                let (k, c) = &list[pick];
                return Some(Held { key: k.clone(), ed: None, sell: (c / 2).max(1), eternal: false });
            }
            pick = side.below(list.len());
        }
        None
    }

    fn price(&self, key: &str) -> f64 {
        self.data.center(key).map_or(5, |c| c.cost) as f64
    }

    /// A joker a consumable makes (Judgement, Wraith, The Soul): in a free slot, else for the
    /// joker whose sale leaves the most, when that beats not using it (card.lua
    /// `Card:can_use_consumeable`: it needs a free slot, so a player sells first)
    fn place(&self, b: &mut Branch, c: Held) {
        if (b.js.len() as i64) < self.slots(&b.js) {
            b.js.push(c);
            return;
        }
        let (v, sold, a) = self.with(&b.js, &b.rest, &c, false);
        if a > self.aim(&b.js, &b.rest, false) {
            b.js = v;
            b.money += sold as f64;
        }
    }

    fn tarot(&self, key: &str) -> Option<&advise::TarotValue> {
        self.tarots.iter().find(|t| t.key == key)
    }

    /// A tarot or spectral used on branch `b` at once, as the module's doc says; `s` draws its
    /// random outcomes (paired: the same trial draws the same). Err: what it can't play.
    fn use_card(&self, b: &mut Branch, key: &str, s: &mut Stream, depth: usize) -> Result<(), String> {
        let run = self.run;
        // the consumable slots free once this one is used (yours stay held)
        let free = (run.consumable_slots - run.consumables.len() as i64).max(0) as usize;
        let mut side = Stream::new(s.0 ^ SIDE);
        match key {
            "c_hermit" => b.money += b.money.clamp(0.0, 20.0),
            "c_temperance" => b.money += b.js.iter().map(|h| h.sell).sum::<i64>().min(50) as f64,
            "c_judgement" | "c_wraith" | "c_soul" => {
                let c = match key {
                    "c_judgement" => self.draw(s, &mut side, 1.0, &b.js),
                    // (common_events.lua `create_card`: Wraith's rarity 0.99, a Rare; The Soul's
                    // a Legendary)
                    "c_wraith" => self.draw_from(&self.pool[3], s, &mut side, &b.js),
                    _ => {
                        let legends: Vec<(String, i64)> = self.data.jokers().filter(|c| c.rarity == Some(4)).map(|c| (c.key.clone(), c.cost)).collect();
                        self.draw_from(&legends, s, &mut side, &b.js)
                    }
                };
                if key == "c_wraith" {
                    b.money = 0.0;
                }
                if let Some(c) = c {
                    self.place(b, c);
                }
            }
            "c_wheel_of_fortune" | "c_hex" | "c_ectoplasm" => {
                let plain: Vec<usize> = (0..b.js.len()).filter(|&i| b.js[i].ed.is_none()).collect();
                // (none without an edition: it can't be used)
                if plain.is_empty() || (key == "c_wheel_of_fortune" && s.unit() >= run.probability_normal / 4.0) {
                    return Ok(());
                }
                let i = plain[s.below(plain.len())];
                b.js[i].ed = Some(match key {
                    "c_hex" => "poly",
                    "c_ectoplasm" => "negative",
                    _ => {
                        let (roll, mut acc) = (s.unit(), 0.0);
                        GUARANTEED_EDITION.iter().find(|(w, _)| {
                            acc += w;
                            roll < acc
                        }).map_or("foil", |&(_, e)| ed_name(e))
                    }
                });
                if key == "c_hex" {
                    b.js = b.js.iter().enumerate().filter(|&(k, h)| k == i || h.eternal).map(|(_, h)| h.clone()).collect();
                }
                if key == "c_ectoplasm" {
                    b.rest.size -= 1;
                }
            }
            "c_ankh" => {
                if b.js.is_empty() || self.slots(&b.js) <= 1 {
                    return Ok(());
                }
                let i = s.below(b.js.len());
                let mut copy = b.js[i].clone();
                b.js = b.js.iter().enumerate().filter(|&(k, h)| k == i || h.eternal).map(|(_, h)| h.clone()).collect();
                if copy.ed == Some("negative") {
                    copy.ed = None;
                }
                b.js.push(copy);
            }
            "c_black_hole" => b.rest.lv.extend(run.hand_levels.keys().cloned()),
            "c_high_priestess" => {
                for _ in 0..free.min(2) {
                    if !self.planets.is_empty() {
                        b.rest.lv.push(self.planets[s.below(self.planets.len())].clone());
                    }
                }
            }
            "c_emperor" | "c_fool" => {
                let made: Vec<String> = if key == "c_emperor" {
                    let all: Vec<&str> = self.data.centers.iter().filter(|c| c.set == "Tarot" && !run.consumables.iter().any(|h| h.key == c.key)).map(|c| c.key.as_str()).collect();
                    (0..free.min(2)).map(|_| all[s.below(all.len())].to_string()).collect()
                } else {
                    run.last_tarot_planet.iter().filter(|k| k.as_str() != "c_fool" && free > 0).cloned().collect()
                };
                for k in made {
                    if let Some(h) = self.data.center(&k).filter(|c| c.set == "Planet").and_then(|c| c.config.get("hand_type")?.as_str()) {
                        b.rest.lv.push(h.to_string());
                    } else if depth < 2 && self.tarot(&k).and_then(|t| t.long_mult).is_some_and(|m| m > 1.0) {
                        self.use_card(b, &k, s, depth + 1)?;
                    } else {
                        b.money += 1.0;
                    }
                }
            }
            _ => {
                let t = self.tarot(key).ok_or_else(|| format!("{key}: not valued by the advice"))?;
                let d = if t.decks.is_empty() {
                    t.deck.clone()
                } else {
                    let (roll, mut acc) = (s.unit(), 0.0);
                    t.decks.iter().find(|(w, _)| {
                        acc += w;
                        roll < acc
                    }).or(t.decks.last()).map(|x| x.1.clone())
                };
                let Some(d) = d else { return Err(format!("{}: not played ({})", t.name, t.note)) };
                if std::env::var_os("BAV_PLAYON_SHOW").is_some() {
                    let changed: Vec<String> = (0..self.fresh.len().min(d.len())).filter(|&i| d[i] != self.fresh[i]).map(|i| format!("{} → {}", self.fresh[i].label(), d[i].label())).collect();
                    let on = self.score(&b.js, &Rest { deck: Some(d.clone()), ..b.rest.clone() });
                    let sorted = self.score(&b.js, &Rest { deck: Some(self.fresh.clone()), ..b.rest.clone() });
                    // the same change through `whatif`'s own cards changed, on the save's deck order
                    let set_cards: Vec<(Card, Card)> = (0..self.fresh.len().min(d.len())).filter(|&i| d[i] != self.fresh[i]).map(|i| (self.fresh[i], d[i])).collect();
                    let plan = whatif::Plan { jokers: Some(b.js.iter().map(Held::spec).collect()), set_cards, ..Default::default() };
                    let w = whatif::run(run, self.data, &plan, SCORE_SIMS, 0x5c0e).map(|o| (o.plan.iter().find(|x| x.label == self.boss).map_or(1.0, |x| x.p_win), o.plan.last().map_or(0.0, |x| x.p_win)));
                    eprintln!(
                        "  {}: {} ({} cards → {}); your board on that deck: {} {:.3}, next ante {:.3} · on your deck in its fixed order {:.3}, {:.3} · whatif with the cards changed {w:.3?}",
                        t.name, changed.join(", "), self.fresh.len(), d.len(), self.boss, on.0, on.1, sorted.0, sorted.1
                    );
                }
                // a second deck change on a first that destroyed or added cards: not played (the
                // card kept unused), so one branch hitting it doesn't leave the option unjudged
                let Some(deck) = compose(&self.fresh, b.rest.deck.as_deref(), &d) else { return Ok(()) };
                b.rest.deck = Some(deck);
                b.rest.size += t.round.0;
                if key == "c_immolate" {
                    b.money += 20.0;
                }
            }
        }
        Ok(())
    }
}

/// `d`, a change of `fresh`, done to `cur` (an earlier change of `fresh`, none: `fresh` itself):
/// cards changed in place carried over one by one, cards added at the end added; `None` when one
/// of them destroyed or added cards and the other isn't only added to
fn compose(fresh: &[Card], cur: Option<&[Card]>, d: &[Card]) -> Option<Vec<Card>> {
    let Some(cur) = cur else { return Some(d.to_vec()) };
    let n = fresh.len();
    if cur.len() < n || d.len() < n {
        return None;
    }
    let mut out = cur.to_vec();
    for i in 0..n {
        if d[i] != fresh[i] {
            out[i] = d[i];
        }
    }
    out.extend_from_slice(&d[n..]);
    Some(out)
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
    rest: Rest,
    money: f64,
    /// the interest cap (Seed Money's, Money Tree's)
    cap: i64,
    boss: f64,
    next: f64,
}

fn judge(path: &str, option: &str, trials: usize, sims: usize) -> String {
    judge_trials(path, option, trials, sims).0
}

/// One policy's trials, each its board's chance at both bosses (this ante's × the next
/// ante's) with the option taken and with leaving (the same for `leave`), trial by trial on the
/// same shops and rounds
#[derive(Clone, Debug)]
struct Trials {
    policy: String,
    take: Vec<f64>,
    leave: Vec<f64>,
}

/// `judge`'s text, and its trials, or why it can't play the option
fn judge_trials(path: &str, option: &str, trials: usize, sims: usize) -> (String, Result<Vec<Trials>, String>) {
    let data = GameData::bundled();
    let run = save::load(std::path::Path::new(path), data).expect("load");
    let mut pool: [Vec<(String, i64)>; 4] = Default::default();
    for k in advise::shop_pool(&run, data) {
        if let Some(c) = data.center(&k) {
            pool[c.rarity.unwrap_or(1).min(3) as usize].push((k.clone(), c.cost));
        }
    }
    let Some(boss) = run.blinds.iter().find(|b| b.slot == "Boss") else { return (format!("{path}: no boss"), Err("no boss".into())) };
    let held0: Vec<Held> = run.jokers.iter().map(|sj| Held { key: sj.key.clone(), ed: sj.edition.map(ed_name), sell: sj.sell_value, eternal: sj.eternal }).collect();
    let negatives = held0.iter().filter(|h| h.ed == Some("negative")).count() as i64;
    // the hands a Celestial pack's planets level: every planet's, a secret hand's once played
    let planets: Vec<String> = data
        .centers
        .iter()
        .filter(|c| c.set == "Planet")
        .filter_map(|c| c.config.get("hand_type")?.as_str().map(str::to_string))
        .filter(|h| run.hand_levels.get(h).is_some_and(|l| l.played > 0 || !matches!(h.as_str(), "Five of a Kind" | "Flush House" | "Flush Five")))
        .collect();
    // the advice's tarot values, only for an option that uses one
    let uses_tarots = ["tarot:", "arcana:", "spectral:"].iter().any(|p| option.starts_with(p));
    let tarots = if uses_tarots { advise::analyze(&run, data, None, &advise::Options::default()).tarots } else { vec![] };
    // (advise.rs `fresh_deck_of`)
    let mut fresh = run.full_deck();
    for c in &mut fresh {
        c.face_down = false;
        c.debuff = false;
    }
    fresh.sort_by_key(Card::order_key);
    let j = Judge {
        run: &run,
        data,
        sims,
        memo: Mutex::new(HashMap::new()),
        scored: Mutex::new(HashMap::new()),
        pool,
        boss: boss.name.clone(),
        base_slots: run.joker_slots - negatives,
        tarots,
        fresh,
        planets: planets.clone(),
    };
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
    let cash = |b: &mut Branch, reward: i64| b.money += reward as f64 + run.money_per_hand + advise::interest(b.money, run.interest_amount, b.cap) as f64;
    // `here`: the shop you're in, its cards left (they're the options), rerolls from its price
    let shop = |b: &mut Branch, pol: Policy, boss_done: bool, seed: u64, here: bool| {
        let mut s = Stream::new(seed);
        let mut side = Stream::new(seed ^ SIDE);
        let mut cards: Vec<Held> = if here { vec![] } else { (0..slots).filter_map(|_| j.draw(&mut s, &mut side, share, &b.js)).collect() };
        let mut rerolls = 0;
        loop {
            let now = j.aim(&b.js, &b.rest, boss_done);
            let mut best: Option<(usize, Vec<Held>, i64, f64)> = None;
            for (i, c) in cards.iter().enumerate().filter(|(_, c)| j.price(&c.key) <= b.money) {
                let (v, sold, a) = j.with(&b.js, &b.rest, c, boss_done);
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
    // the shop's price of a card it sells, else the one given, else its base price
    // the price given (a card you hold or pick from a pack: 0), else the shop's, else its base price
    let shop_cost = |key: &str, given: Option<&str>| -> f64 {
        given.and_then(|g| g.parse().ok()).or_else(|| run.shop.as_ref().and_then(|s| s.other_cards.iter().chain(&s.vouchers).find(|x| x.key == key)).map(|x| x.cost as f64)).unwrap_or_else(|| j.price(key))
    };
    // An Arcana or Spectral pack's cards (card.lua `Card:open`; common_events.lua `create_card`:
    // its pool leaves out a card you hold unless you have Showman, `get_current_pool`; The Soul
    // and Black Hole come only by the 0.3% roll, game.lua's `hidden`)
    let showman = run.jokers.iter().any(|j| j.key == "j_ring_master");
    let deal = |set: &str, n: usize, s: &mut Stream| -> Vec<String> {
        let omen = set == "Tarot" && run.vouchers.iter().any(|v| v == "v_omen_globe");
        let mut shown: Vec<String> = vec![];
        for _ in 0..n {
            let set = if omen && s.unit() > 0.8 { "Spectral" } else { set };
            let soul = s.unit() > 0.997;
            let hole = set == "Spectral" && s.unit() > 0.997;
            let forced = if soul { Some("c_soul") } else if hole { Some("c_black_hole") } else { None };
            let free: Vec<&str> = data
                .centers
                .iter()
                .filter(|c| c.set == set && !matches!(c.key.as_str(), "c_soul" | "c_black_hole"))
                .filter(|c| showman || (!run.consumables.iter().any(|h| h.key == c.key) && !shown.contains(&c.key)))
                .map(|c| c.key.as_str())
                .collect();
            match forced {
                Some(k) => shown.push(k.to_string()),
                None if !free.is_empty() => shown.push(free[s.below(free.len())].to_string()),
                None => {}
            }
        }
        shown
    };
    let play = |pol: Policy, take: bool, t: usize| -> Result<Branch, String> {
        let mut b = Branch { js: held0.clone(), rest: Rest::default(), money: run.dollars, cap: run.interest_cap, boss: 0.0, next: 0.0 };
        if take {
            if let Some(spec) = option.strip_prefix("joker:") {
                let (k, ed) = match spec.split_once(':') {
                    Some((k, e)) => (k, Some(match e { "foil" => "foil", "holo" => "holo", "poly" | "polychrome" => "poly", "negative" => "negative", other => return Err(format!("unknown edition {other}")) })),
                    None => (spec, None),
                };
                // the shop's card of that key and edition, else of that key
                let shop = run.shop.as_ref().map(|s| s.jokers.as_slice()).unwrap_or_default();
                let card = shop.iter().find(|x| x.key == k && x.edition.map(ed_name) == ed).or_else(|| shop.iter().find(|x| x.key == k));
                let cost = card.map_or_else(|| j.price(k), |x| x.cost as f64);
                let (v, sold, _) = j.with(&b.js, &b.rest, &Held { key: k.to_string(), ed, sell: ((cost as i64) / 2).max(1), eternal: false }, false);
                b.js = v;
                b.money += sold as f64 - cost;
            } else if let Some(r) = option.strip_prefix("sell:") {
                // `sell:<key>[:<place in your row, from 1>]`
                let (k, place) = r.split_once(':').map_or((r, None), |(k, p)| (k, p.parse::<usize>().ok()));
                let at = place.and_then(|p| p.checked_sub(1)).filter(|&i| b.js.get(i).is_some_and(|h| h.key == k));
                if let Some(i) = at.or_else(|| b.js.iter().position(|h| h.key == k)) {
                    b.money += b.js.remove(i).sell as f64;
                }
            } else if let Some((hand, cost)) = option.strip_prefix("planet:").and_then(|r| r.rsplit_once(':')) {
                b.money -= cost.parse::<f64>().unwrap_or(3.0);
                b.rest.lv.push(hand.to_string());
            } else if let Some(r) = option.strip_prefix("tarot:") {
                let (k, given) = r.split_once(':').map_or((r, None), |(k, c)| (k, Some(c)));
                b.money -= shop_cost(k, given);
                j.use_card(&mut b, k, &mut Stream::new(0x7a0 ^ (t as u64).wrapping_mul(977)), 0)?;
            } else if let Some((k, cost)) = option.strip_prefix("voucher:").and_then(|r| r.split_once(':')) {
                // (card.lua `Card:apply_to_run`: Seed Money's and Money Tree's interest cap)
                b.money -= shop_cost(k, Some(cost));
                b.cap = match k {
                    "v_seed_money" => b.cap.max(50),
                    "v_money_tree" => b.cap.max(100),
                    _ => return Err(format!("{k}: a voucher the judge doesn't play")),
                };
            } else if let Some((set, r)) = option.strip_prefix("arcana:").map(|r| ("Tarot", r)).or_else(|| option.strip_prefix("spectral:").map(|r| ("Spectral", r))) {
                let parts: Vec<&str> = r.split(':').collect();
                let n = parts.first().and_then(|x| x.parse().ok()).unwrap_or(3usize);
                b.money -= parts.get(1).and_then(|x| x.parse().ok()).unwrap_or(4.0);
                let picks = parts.get(2).and_then(|x| x.parse().ok()).unwrap_or(1usize);
                let mut s = Stream::new(0x7a0 ^ (t as u64).wrapping_mul(977));
                let mut shown = deal(set, n, &mut s);
                for _ in 0..picks {
                    // the advice's pick: the card it values most by Ante 8, none under ×1.00
                    let best = shown.iter().enumerate().filter_map(|(i, k)| Some((i, j.tarot(k)?.long_mult?))).filter(|x| x.1 > 1.0).max_by(|x, y| x.1.total_cmp(&y.1));
                    let Some((i, _)) = best else { break };
                    let k = shown.remove(i);
                    j.use_card(&mut b, &k, &mut s, 0)?;
                }
            } else if let Some(r) = option.strip_prefix("celestial:") {
                // a Celestial pack, `celestial:<planets>:<price>[:<picks>]`: `n` planets of the
                // hands you can level, the best picked (a Mega pack: two)
                let parts: Vec<&str> = r.split(':').collect();
                let n = parts.first().and_then(|x| x.parse().ok()).unwrap_or(3usize);
                b.money -= parts.get(1).and_then(|x| x.parse::<f64>().ok()).unwrap_or(4.0);
                let picks = parts.get(2).and_then(|x| x.parse().ok()).unwrap_or(1usize);
                let mut s = Stream::new(0xce1 ^ (t as u64).wrapping_mul(977));
                let mut hands = planets.clone();
                let mut shown = vec![];
                for _ in 0..n.min(hands.len()) {
                    shown.push(hands.remove(s.below(hands.len())));
                }
                for _ in 0..picks {
                    let now = j.aim(&b.js, &b.rest, false);
                    let pick = shown.iter().enumerate().map(|(i, h)| {
                        let mut r = b.rest.clone();
                        r.lv.push(h.clone());
                        (j.aim(&b.js, &r, false), r, i)
                    }).max_by(|x, y| x.0.total_cmp(&y.0));
                    let Some((_, r, i)) = pick.filter(|p| p.0 > now) else { break };
                    b.rest = r;
                    shown.remove(i);
                }
            } else {
                if option != "reroll" && !option.starts_with("pack:") {
                    return Err(format!("unknown option {option}"));
                }
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
                    let now = j.aim(&b.js, &b.rest, false);
                    let mut best: Option<(usize, Vec<Held>, i64, f64, f64)> = None;
                    for (i, c) in cards.iter().enumerate() {
                        let price = if pay { j.price(&c.key) } else { 0.0 };
                        if price > b.money {
                            continue;
                        }
                        let (v, sold, a) = j.with(&b.js, &b.rest, c, false);
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
        b.boss = j.score(&b.js, &b.rest).0;
        next_shop(&mut b, boss.reward, true);
        for &r in &next_ante {
            next_shop(&mut b, r, true);
        }
        b.next = j.score(&b.js, &b.rest).1;
        Ok(b)
    };
    let (b0, n0) = j.score(&held0, &Rest::default());
    let mut out = format!("{path} {option}: ante {} ${} · now {} {b0:.3}, next ante {n0:.3}\n", run.ante, run.dollars, boss.name);
    let policies = [
        Policy { name: "spend", reserve: 0.0, big: 0.0, rerolls: 3 },
        Policy { name: "cap", reserve: run.interest_cap as f64, big: 0.05, rerolls: 3 },
    ];
    // the money's own play-on also rerolls up to 8 times a shop, as the advice's walk may
    let more = [Policy { name: "spend8", reserve: 0.0, big: 0.0, rerolls: 8 }];
    let policies: Vec<Policy> = policies.into_iter().chain(if option == "money" || option == "leave" { more.to_vec() } else { vec![] }).collect();
    let mut kept: Vec<Trials> = vec![];
    for pol in policies {
        if option == "leave" || option == "money" {
            // leaving alone: what the shops ahead add with the money you hold, played on
            let res: Vec<Branch> = (0..trials).map(|t| play(pol, false, t).expect("leaving plays")).collect();
            let (db, seb) = mean_se(&res.iter().map(|b| b.boss - b0).collect::<Vec<_>>());
            let (dn, sen) = mean_se(&res.iter().map(|b| b.next - n0).collect::<Vec<_>>());
            let (m, _) = mean_se(&res.iter().map(|b| b.money).collect::<Vec<_>>());
            out += &format!("  {:5}: this boss {db:+.3} ± {seb:.3}, next {dn:+.3} ± {sen:.3} from the shops ahead; money at the next boss ${m:.1}\n", pol.name);
            let both: Vec<f64> = res.iter().map(|b| b.boss * b.next).collect();
            kept.push(Trials { policy: pol.name.into(), take: both.clone(), leave: both });
            continue;
        }
        let res: Result<Vec<(Branch, Branch)>, String> = (0..trials).map(|t| Ok((play(pol, true, t)?, play(pol, false, t)?))).collect();
        let res = match res {
            Ok(r) => r,
            Err(e) => {
                // what it can't play isn't judged
                out += &format!("  can't judge: {e}\n");
                return (out, Err(e));
            }
        };
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
        kept.push(Trials { policy: pol.name.into(), take: res.iter().map(|(a, _)| a.boss * a.next).collect(), leave: res.iter().map(|(_, b)| b.boss * b.next).collect() });
    }
    (out, Ok(kept))
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
    let _run = stamp::start("shop_playon", None);
    let Some(list) = std::env::var_os("BAV_PLAYON") else { panic!("set BAV_PLAYON to a list of `<state.jkr> <option>` lines") };
    let num = |k: &str, d: usize| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
    let (trials, sims) = (num("BAV_PLAYON_TRIALS", 24), num("BAV_PLAYON_SIMS", 150));
    for line in std::fs::read_to_string(list).unwrap().lines().filter(|l| !l.trim().is_empty()) {
        let (path, option) = line.trim().split_once(' ').expect("`<state.jkr> <option>`");
        print!("{}", judge(path, option.trim(), trials, sims));
    }
}

/// Where the judge pass keeps what it played (`BAV_SHOP_JUDGE_CACHE`, else
/// `shop_judge_cache.tsv` in the live page's folder): a row per option played, keyed by the
/// code it was played on (the library's inputs and this file), the state, the option, the
/// trials and the rounds, so a check judged again, or another check moving the same pick on the
/// same code, plays nothing twice
fn cache_path() -> std::path::PathBuf {
    std::env::var_os("BAV_SHOP_JUDGE_CACHE").map(std::path::PathBuf::from).unwrap_or_else(|| {
        let dir = balatro_advisor::calibration::default_dir().unwrap_or_else(std::env::temp_dir);
        let _ = std::fs::create_dir_all(&dir);
        dir.join("shop_judge_cache.tsv")
    })
}

/// The code the judge plays on: the library's inputs and this file's text
fn judge_code() -> String {
    let fnv = |s: &str| s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    format!("{}-{:08x}", stamp::source_hash(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))), fnv(include_str!("shop_playon.rs")) as u32)
}

type Played = Result<Vec<Trials>, String>;

/// The cache's rows for `code`: (state, option, trials, rounds) → what was played
fn read_cache(code: &str) -> HashMap<(String, String, usize, usize), Played> {
    let mut out: HashMap<(String, String, usize, usize), Played> = HashMap::new();
    let nums = |s: &str| s.split(',').filter(|x| !x.is_empty()).map(|x| x.parse::<f64>().unwrap_or(f64::NAN)).collect::<Vec<f64>>();
    for l in std::fs::read_to_string(cache_path()).unwrap_or_default().lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 7 || f[0] != code {
            continue;
        }
        let (Ok(t), Ok(s)) = (f[3].parse(), f[4].parse()) else { continue };
        let k = (f[1].to_string(), f[2].to_string(), t, s);
        if f[5] == "ERR" {
            out.insert(k, Err(f[6].to_string()));
            continue;
        }
        // (a row cut short by a killed run is skipped: its option is played again)
        let (take, leave) = (nums(f[6]), nums(f.get(7).copied().unwrap_or("")));
        if take.is_empty() || take.len() != leave.len() || take.iter().chain(&leave).any(|x| x.is_nan()) {
            continue;
        }
        if let Ok(v) = out.entry(k).or_insert_with(|| Ok(vec![])) {
            v.push(Trials { policy: f[5].to_string(), take, leave });
        }
    }
    out
}

fn write_cache(code: &str, name: &str, option: &str, trials: usize, sims: usize, played: &Played) {
    use std::io::Write;
    let head = format!("{code}\t{name}\t{option}\t{trials}\t{sims}");
    let csv = |v: &[f64]| v.iter().map(|x| format!("{x:.6}")).collect::<Vec<_>>().join(",");
    let rows = match played {
        Err(e) => format!("{head}\tERR\t{}\n", e.replace(['\t', '\n'], " ")),
        Ok(ts) => ts.iter().map(|t| format!("{head}\t{}\t{}\t{}\n", t.policy, csv(&t.take), csv(&t.leave))).collect(),
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(cache_path()) {
        let _ = f.write_all(rows.as_bytes());
    }
}

/// The judged policies (the two every option is played by, "spend8" left out: only leaving has
/// it), and per trial the mean over them of `f`'s gain against leaving
fn per_trial(a: &[Trials], b: Option<&[Trials]>) -> Vec<f64> {
    let gain = |t: &Trials, i: usize| t.take[i] - t.leave[i];
    let pols: Vec<&Trials> = a.iter().filter(|t| t.policy != "spend8").collect();
    let n = pols.iter().map(|t| t.take.len()).min().unwrap_or(0);
    (0..n)
        .map(|i| {
            let v: Vec<f64> = pols
                .iter()
                .map(|t| {
                    let old = b.and_then(|b| b.iter().find(|o| o.policy == t.policy)).map_or(0.0, |o| gain(o, i));
                    gain(t, i) - old
                })
                .collect();
            v.iter().sum::<f64>() / v.len().max(1) as f64
        })
        .collect()
}

/// The shop judge on a change's moved top picks (gap 17's "How the picks were judged"): for
/// each state whose top option moved between two runs of `shop_picks.rs` (`BAV_SHOP_PICKS_BEFORE`
/// and `BAV_SHOP_PICKS_AFTER`, each made on its own code: a base's in a worktree), the old and
/// the new top option each played on against leaving (`judge_trials`: both policies, trial by
/// trial on the same shops and rounds), and the new less the old, trial by trial, averaged
/// over the two policies: better or worse where that's over two standard errors from zero,
/// else within. A pick moves when its judge option changes (its label where either can't be
/// played). What the judge can't play is counted apart with its reason. Slow: an option played
/// is 1-10 minutes at the default 24 trials (`BAV_PLAYON_TRIALS`, `BAV_PLAYON_SIMS` as for
/// `shop_playon`), kept in the cache (`cache_path`) so nothing is played twice on the same
/// code; `BAV_ONLY` as for the picks.
/// `BAV_SHOP_PICKS_BEFORE=/tmp/before BAV_SHOP_PICKS_AFTER=/tmp/after cargo test --release
/// --test shop_playon -- --ignored shop_judge --nocapture`
#[test]
#[ignore]
fn shop_judge() {
    let _run = stamp::start("shop_judge", None);
    let (Some(before), Some(after)) = (std::env::var_os("BAV_SHOP_PICKS_BEFORE"), std::env::var_os("BAV_SHOP_PICKS_AFTER")) else { panic!("set BAV_SHOP_PICKS_BEFORE and BAV_SHOP_PICKS_AFTER") };
    let num = |k: &str, d: usize| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d);
    let (trials, sims) = (num("BAV_PLAYON_TRIALS", 24), num("BAV_PLAYON_SIMS", 150));
    assert!(trials >= 2, "BAV_PLAYON_TRIALS: at least 2 trials, for a standard error");
    let top = |dir: &std::ffi::OsStr| -> std::collections::BTreeMap<String, shop_picks::Row> {
        shop_picks::read(std::path::Path::new(dir)).into_iter().filter(|r| r.1 <= 1).map(|r| (r.0.clone(), r)).collect()
    };
    let (before, after) = (top(&before), top(&after));
    let paths: HashMap<String, Option<std::path::PathBuf>> = shop_picks::states().into_iter().collect();
    let id = |r: &shop_picks::Row| if r.2 == "-" { format!("label {}", r.3) } else { r.2.clone() };
    let moved: Vec<(&String, &shop_picks::Row, &shop_picks::Row)> =
        after.iter().filter(|(k, _)| paths.contains_key(*k)).filter_map(|(k, new)| before.get(k).filter(|old| id(old) != id(new)).map(|old| (k, old, new))).collect();
    let both = after.keys().filter(|k| before.contains_key(*k) && paths.contains_key(*k)).count();
    println!("{both} states in both, {} top picks moved ({trials} trials a policy, {sims} rounds)", moved.len());
    let code = judge_code();
    let mut cache = read_cache(&code);
    // (the state is keyed by its name and its file's hash: the same name from another history
    // folder, or a file saved again under it, is played again)
    let mut play = |name: &str, path: &std::path::Path, option: &str| -> Played {
        let fnv = |b: &[u8]| b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100_0000_01b3));
        let name = &format!("{name}@{:016x}", fnv(&std::fs::read(path).unwrap_or_default()));
        let k = (name.to_string(), option.to_string(), trials, sims);
        if let Some(p) = cache.get(&k) {
            return p.clone();
        }
        let t = std::time::Instant::now();
        let played = judge_trials(&path.to_string_lossy(), option, trials, sims).1;
        eprintln!("{name} {option}: played in {:.0?}", t.elapsed());
        write_cache(&code, name, option, trials, sims, &played);
        cache.insert(k, played.clone());
        played
    };
    let mut rows: Vec<(f64, String)> = vec![];
    let mut unjudged: Vec<String> = vec![];
    let (mut better, mut worse, mut within, mut up, mut down, mut even, mut sum) = (0, 0, 0, 0, 0, 0, 0.0);
    for (name, old, new) in &moved {
        let line = format!("{name}: {} -> {}", if old.3.is_empty() { &old.2 } else { &old.3 }, if new.3.is_empty() { &new.2 } else { &new.3 });
        let Some(Some(path)) = paths.get(*name) else {
            unjudged.push(format!("{line}: the state isn't on this machine"));
            continue;
        };
        if let Some(r) = [old, new].iter().find(|r| r.2 == "-") {
            unjudged.push(format!("{line}: {} ({})", r.3, if r.4.is_empty() { "not played" } else { &r.4 }));
            continue;
        }
        let (a, b) = (play(name, path, &old.2), play(name, path, &new.2));
        let (a, b) = match (a, b) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                unjudged.push(format!("{line}: {e}"));
                continue;
            }
        };
        let (d, se) = mean_se(&per_trial(&b, Some(&a)));
        let (vo, so) = mean_se(&per_trial(&a, None));
        let (vn, sn) = mean_se(&per_trial(&b, None));
        let verdict = if d - 2.0 * se > 0.0 {
            better += 1;
            "BETTER"
        } else if d + 2.0 * se < 0.0 {
            worse += 1;
            "WORSE"
        } else {
            within += 1;
            "within"
        };
        sum += d;
        match d.total_cmp(&0.0) {
            std::cmp::Ordering::Greater => up += 1,
            std::cmp::Ordering::Less => down += 1,
            std::cmp::Ordering::Equal => even += 1,
        }
        rows.push((d, format!("{verdict:6} {line} ({} -> {}): {d:+.3} ± {se:.3} (against leaving {vo:+.3} ± {so:.3} -> {vn:+.3} ± {sn:.3})", old.2, new.2)));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, r) in &rows {
        println!("{r}");
    }
    for u in &unjudged {
        println!("can't   {u}");
    }
    println!(
        "moved {}, judged {}: {better} better, {worse} worse, {within} within two standard errors, summing {sum:+.3} ({up} up, {down} down, {even} even); {} not judged",
        moved.len(),
        rows.len(),
        unjudged.len()
    );
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
    // (on a board holding a joker that saves a lost round, `survive` also has the save's lift,
    // `save_lift`: none of the states measured so far holds one)
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
    let _run = stamp::start("money_finds", None);
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
