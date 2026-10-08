//! The scoring pass: a port of `G.FUNCS.evaluate_play` (state_events.lua) and the
//! scoring contexts of `Card:calculate_joker` (card.lua). Comments name the game
//! code each block mirrors.

use serde::{Deserialize, Serialize};

use super::hand::{self, HandInfo, HandType, RuleFlags, card_id, is_face, is_suit};
use super::joker::{Joker, Kind};
use super::rng::{Rolls, Unlucky};
use crate::model::{Card, Edition, Enhancement, Seal, Suit};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub level: i64,
    pub chips: f64,
    pub mult: f64,
    pub s_chips: f64,
    pub s_mult: f64,
    pub l_chips: f64,
    pub l_mult: f64,
    pub played: i64,
    pub played_this_round: i64,
    pub visible: bool,
}

impl Level {
    /// `level_up_hand`: level changes recompute chips and mult from the base values.
    pub fn with_level(mut self, level: i64) -> Level {
        self.level = level.max(0);
        self.mult = (self.s_mult + self.l_mult * (self.level - 1) as f64).max(1.0);
        self.chips = (self.s_chips + self.l_chips * (self.level - 1) as f64).max(0.0);
        self
    }

    /// Level 1 values from `G.GAME.hands` defaults (game.lua `init_game_object`).
    pub fn base(h: HandType) -> Level {
        let (s_chips, s_mult, l_chips, l_mult) = match h {
            HandType::FlushFive => (160., 16., 50., 3.),
            HandType::FlushHouse => (140., 14., 40., 4.),
            HandType::FiveOfAKind => (120., 12., 35., 3.),
            HandType::StraightFlush => (100., 8., 40., 4.),
            HandType::FourOfAKind => (60., 7., 30., 3.),
            HandType::FullHouse => (40., 4., 25., 2.),
            HandType::Flush => (35., 4., 15., 2.),
            HandType::Straight => (30., 4., 30., 3.),
            HandType::ThreeOfAKind => (30., 3., 20., 2.),
            HandType::TwoPair => (20., 2., 20., 1.),
            HandType::Pair => (10., 2., 15., 1.),
            HandType::HighCard => (5., 1., 10., 1.),
        };
        let visible = !matches!(h, HandType::FlushFive | HandType::FlushHouse | HandType::FiveOfAKind);
        Level { level: 1, chips: s_chips, mult: s_mult, s_chips, s_mult, l_chips, l_mult, played: 0, played_this_round: 0, visible }
    }
}

/// Boss effects that change a single hand's score.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BlindRules {
    pub key: String,
    pub disabled: bool,
    /// The Eye: bitmask of hands already played this round.
    pub eye_seen: u16,
    /// The Mouth: the hand locked in this round.
    pub mouth_only: Option<HandType>,
}

impl BlindRules {
    fn active(&self, key: &str) -> bool {
        !self.disabled && self.key == key
    }
}

/// Everything besides the cards that scoring depends on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub jokers: Vec<Joker>,
    pub joker_slots: i64,
    pub levels: [Level; 12],
    /// Planet cards held (Observatory).
    pub planets_held: Vec<HandType>,
    pub observatory: bool,
    /// Hands left *before* this play (Dusk/Acrobat look at the count after it).
    pub hands_left: i64,
    pub discards_left: i64,
    /// Discards used this round (`G.GAME.current_round.discards_used`).
    #[serde(default)]
    pub discards_used: i64,
    pub dollars: f64,
    pub skips: i64,
    /// Hands played this run before this one (Loyalty Card).
    pub hands_played: i64,
    pub tarots_used: i64,
    pub starting_deck_size: i64,
    /// Full deck size (`#G.playing_cards`).
    pub playing_cards: i64,
    /// Cards left in the draw pile while this hand scores (Blue Joker).
    pub deck_remaining: i64,
    pub steel_tally: i64,
    pub stone_tally: i64,
    pub driver_tally: i64,
    /// `G.GAME.probabilities.normal`.
    pub probability: f64,
    pub idol: Option<(u8, Suit)>,
    pub ancient_suit: Option<Suit>,
    pub most_played: Option<HandType>,
    pub blind: BlindRules,
    /// Mail-In Rebate's rank this round (it pays $5 per discarded card of it).
    /// Consumable slots free for the planets Blue Seal cards held at round end make.
    pub planet_slots: i64,
    /// What a round is worth beyond winning it, for the round simulation's own choices
    /// (none: it just plays to win).
    #[serde(skip)]
    pub goals: Option<crate::sim::RoundGoals>,
    pub mail_rank: Option<u8>,
    /// Castle's suit this round (it grows from discarded cards of it).
    #[serde(default)]
    pub castle_suit: Option<Suit>,
    pub plasma: bool,
}

impl Board {
    /// A plain board: level 1 hands, no jokers, 4 hands / 3 discards, $0.
    pub fn empty() -> Board {
        Board {
            jokers: Vec::new(),
            joker_slots: 5,
            levels: HandType::ALL.map(Level::base),
            planets_held: Vec::new(),
            observatory: false,
            hands_left: 4,
            discards_left: 3,
            discards_used: 0,
            dollars: 0.0,
            skips: 0,
            hands_played: 0,
            tarots_used: 0,
            starting_deck_size: 52,
            playing_cards: 52,
            deck_remaining: 44,
            steel_tally: 0,
            stone_tally: 0,
            driver_tally: 0,
            probability: 1.0,
            idol: None,
            ancient_suit: None,
            most_played: None,
            blind: BlindRules::default(),
            plasma: false,
            mail_rank: None,
            castle_suit: None,
            planet_slots: 2,
            goals: None,
        }
    }

    /// `find_joker(name)`: only non-debuffed copies count.
    fn has(&self, k: Kind) -> bool {
        self.jokers.iter().any(|j| j.kind == k && !j.debuff)
    }

    pub fn rule_flags(&self) -> RuleFlags {
        RuleFlags {
            four_fingers: self.has(Kind::FourFingers),
            shortcut: self.has(Kind::Shortcut),
            smeared: self.has(Kind::Smeared),
            splash: self.has(Kind::Splash),
            pareidolia: self.has(Kind::Pareidolia),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub source: String,
    pub chips: f64,
    pub mult: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub hand: HandType,
    pub scoring: Vec<usize>,
    pub chips: f64,
    pub mult: f64,
    /// `floor(chips × mult)`, what the round total gains.
    pub score: f64,
    /// Money earned while scoring (gold seals, Lucky cards, Golden Ticket, …).
    pub dollars: f64,
    /// The boss blocked this hand (Psychic, Eye, Mouth): it scores 0.
    pub debuffed_hand: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<Step>,
    /// What the hand leaves for the next one (`Board::after_hand`): each joker's state, the
    /// hand's level (played once more; Space Joker's level-up, The Arm's level-down) and
    /// your money (The Tooth and The Ox included).
    #[serde(skip)]
    pub jokers: Vec<JokerState>,
    #[serde(skip)]
    pub level: Level,
    #[serde(skip)]
    pub money: f64,
    /// Whether a card held in hand took part: its own effect or a joker's on it (Steel, Baron,
    /// Raised Fist, …), or a joker looked at the held cards (Blackboard). When none did, the
    /// same play with fewer cards held scores the same (the held cards are read nowhere else).
    #[serde(skip)]
    pub held_used: bool,
    /// The order to play the cards in (indices into the played cards: the scoring ones
    /// arranged, the rest after them), when it scores more than the order given. `None`: the
    /// order given is as good.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub played_order: Option<Vec<usize>>,
    /// The order to hold the cards kept in hand in, left to right (indices into the held
    /// cards), when it scores more than the order given. `None`: the order given is as good.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_order: Option<Vec<usize>>,
    /// Orders proposed for the parts whose cards' maps aren't exact (`Pass::arrangement`)
    #[serde(skip)]
    pub(crate) proposed: (Option<Vec<usize>>, Option<Vec<usize>>),
}

/// One step of a card's effect on the score: chips added, Mult added, then ×Mult (the order
/// `evaluate_play` applies an effect's fields in).
#[derive(Clone, Copy)]
struct Op {
    chips: f64,
    mult: f64,
    x: f64,
}

/// The steps of every card in one part of the pass (the played cards or the held ones), in
/// the order they were worked out. `labels` (a trace step after the op, or none) only when
/// tracing.
#[derive(Default)]
struct Ops {
    ops: Vec<Op>,
    /// where each card's steps end in `ops`
    ends: Vec<usize>,
    labels: Vec<Option<String>>,
}

thread_local! {
    /// `Ops` buffers reused from one scoring pass to the next (a pass runs millions of times in
    /// simulated rounds; allocating them each time cost about a sixth of the time)
    static SPARE_OPS: std::cell::RefCell<Vec<Ops>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Ops {
    fn take() -> Ops {
        SPARE_OPS.with(|v| v.borrow_mut().pop()).unwrap_or_default()
    }

    fn give_back(mut self) {
        self.ops.clear();
        self.ends.clear();
        self.labels.clear();
        SPARE_OPS.with(|v| v.borrow_mut().push(self));
    }

    fn card(&self, i: usize) -> std::ops::Range<usize> {
        if i == 0 { 0..self.ends[0] } else { self.ends[i - 1]..self.ends[i] }
    }

    /// The card's steps as one map of Mult: `mult → x·mult + b` (chips only add, so their
    /// order never matters).
    fn map(&self, i: usize) -> (f64, f64) {
        self.ops[self.card(i)].iter().fold((1.0, 0.0), |(x, b), o| (x * o.x, (b + o.mult) * o.x))
    }

    /// Mult after applying every card's steps in `order`, from `mult`.
    fn mult_after(&self, order: &[usize], mut mult: f64) -> f64 {
        for &i in order {
            for o in &self.ops[self.card(i)] {
                mult += o.mult;
                mult *= o.x;
            }
        }
        mult
    }

    /// The order of the cards that leaves the most Mult, when it's more than the order they
    /// came in. Each card's steps are a map `mult → x·mult + b` (x ≥ 1, b ≥ 0 for every card
    /// effect in the game), and swapping two neighbours helps exactly when the later one has
    /// the larger b / (x − 1): sorted by that, no swap helps (pure +Mult first, pure ×Mult
    /// last; the exchange argument).
    fn best_order(&self, mult: f64) -> Option<Vec<usize>> {
        let n = self.ends.len();
        let key = |i: usize| {
            let (x, b) = self.map(i);
            if x == 1.0 { f64::INFINITY } else { b / (x - 1.0) }
        };
        // already in that order (the usual case): nothing to arrange
        let mut prev = f64::INFINITY;
        if (0..n).all(|i| {
            let k = key(i);
            let sorted = prev >= k;
            prev = k;
            sorted
        }) {
            return None;
        }
        let keys: Vec<f64> = (0..n).map(key).collect();
        let key = |i: usize| keys[i];
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &c| key(c).total_cmp(&key(a)));
        let given: Vec<usize> = (0..n).collect();
        (order != given && self.mult_after(&order, mult) > self.mult_after(&given, mult)).then_some(order)
    }
}

/// Effect a joker returns in one context. `x` = 1 means no ×Mult.
#[derive(Debug, Clone, Copy)]
struct Eff {
    chips: f64,
    mult: f64,
    x: f64,
    h_mult: f64,
    dollars: f64,
    reps: u32,
    level_up: bool,
}

const NONE: Eff = Eff { chips: 0.0, mult: 0.0, x: 1.0, h_mult: 0.0, dollars: 0.0, reps: 0, level_up: false };

fn eff() -> Option<Eff> {
    Some(NONE)
}

#[derive(Clone, Copy)]
enum Ctx {
    Before,
    PlayIndividual(usize),
    HeldIndividual(usize),
    PlayRepetition(usize),
    /// whether the held card produced any effect this pass (Mime's condition)
    HeldRepetition(bool),
    OtherJoker(usize),
    Main,
}

/// Values a joker can change mid-hand (the game mutates `self.ability` in place).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct JokerState {
    mult: f64,
    x_mult: f64,
    extra_chips: f64,
}

struct Pass<'a, R: Rolls + ?Sized> {
    b: &'a Board,
    rolls: &'a mut R,
    flags: RuleFlags,
    /// copied only when a joker changes a played card (Hiker, Midas Mask, Vampire)
    played: std::borrow::Cow<'a, [Card]>,
    held: &'a [Card],
    info: HandInfo,
    js: Vec<JokerState>,
    hands_left: i64,
    dollars: f64,
    earned: f64,
    lucky_trigger: bool,
    blind_triggered: bool,
    level: Level,
    trace: Option<Vec<Step>>,
    /// Whether to propose an order of the cards (`Outcome::played_order`, `held_order`)
    arrange: bool,
    /// inside the played or held cards' part, and whether a roll was asked for there
    in_cards: bool,
    card_rolls: bool,
    /// whether an effect read where a card sits (Photograph, Hanging Chad, Raised Fist):
    /// cards' maps then hold only for the order given
    reads_place: bool,
    /// orders to check as played (`score`): parts of the pass whose maps aren't exact
    proposed: (Option<Vec<usize>>, Option<Vec<usize>>),
    played_order: Option<Vec<usize>>,
    held_order: Option<Vec<usize>>,
    /// `Outcome::held_used`
    held_used: bool,
}

/// The joker whose effect joker `j` has: itself, or what a Blueprint (its right neighbour) or
/// Brainstorm (the leftmost joker) copies, through chains of copies (as `Pass::calc`); none if
/// a joker on the way is debuffed or the copies loop. It doesn't model the game's
/// `not context.blueprint` gates: an effect that copies don't trigger must check for that
/// itself (Mail-In Rebate and Faceless Joker have no such gate).
pub fn effective_joker(b: &Board, j: usize) -> Option<&Joker> {
    let n = b.jokers.len();
    let mut cur = j;
    for _ in 0..=n {
        let jk = b.jokers.get(cur)?;
        if jk.debuff {
            return None;
        }
        let t = match jk.kind {
            Kind::Blueprint => cur + 1,
            Kind::Brainstorm => 0,
            _ => return Some(jk),
        };
        if t >= n || t == cur {
            return None;
        }
        cur = t;
    }
    None
}

/// What `n` of `ev` change on the board: the jokers that grow from it (`Joker::mult_from`) and
/// the counters the game keeps (`G.GAME.skips` for a skipped blind, which Throwback scores
/// from, ×`extra` each: card.lua `calculate_joker`; a whole count, so `n` is rounded there).
impl Board {
    pub fn after(&mut self, ev: crate::engine::RunEvent, n: f64) {
        for j in self.jokers.iter_mut() {
            j.grow_from(ev, n);
        }
        if ev == crate::engine::RunEvent::SkipBlind {
            self.skips += n.round() as i64;
        }
    }

    /// Whether `ev` changes how the board scores
    pub fn changes_with(&self, ev: crate::engine::RunEvent) -> bool {
        self.jokers.iter().any(|j| j.mult_from(ev) != 0.0 || j.xmult_from(ev) != 0.0)
    }
}

/// Money a discard pays (card.lua `Card:calculate_joker`, discard context; copies pay too
/// unless noted): Mail-In Rebate `extra` per discarded card of the round's rank (`get_id`: not
/// debuffed, not Stone); Faceless Joker `extra.dollars` when `extra.faces` or more of the
/// discarded cards are faces (`is_face`: Pareidolia makes every card one, debuffed cards
/// aren't); Trading Card (not a copy) `extra` for a first discard of one card.
pub fn discard_money(b: &Board, discarded: &[Card]) -> f64 {
    let pareidolia = b.has(Kind::Pareidolia);
    let ranked = b.mail_rank.map_or(0, |r| discarded.iter().enumerate().filter(|(i, c)| !c.debuff && card_id(c, *i) == i32::from(r)).count());
    let faces = discarded.iter().enumerate().filter(|(i, c)| is_face(c, *i, pareidolia, false)).count();
    let copies = (0..b.jokers.len()).filter_map(|j| effective_joker(b, j)).map(|jk| match jk.key.as_str() {
        "j_mail" => jk.extra.n * ranked as f64,
        "j_faceless" if faces as f64 >= jk.extra.faces => jk.extra.dollars,
        _ => 0.0,
    });
    let own = b.jokers.iter().filter(|jk| !jk.debuff).map(|jk| match jk.key.as_str() {
        "j_trading" if b.discards_used <= 0 && discarded.len() == 1 => jk.extra.n,
        _ => 0.0,
    });
    copies.chain(own).sum()
}

/// Money a won round pays at its end that depends on how it was played (card.lua
/// `Card:calculate_dollar_bonus`, not debuffed): Delayed Gratification `extra` for each
/// discard left when none was used. Fixed payouts (Golden Joker, Rocket…) are valued per
/// ante instead (`income_per_ante`).
pub fn won_round_money(b: &Board) -> f64 {
    let unused = if b.discards_used == 0 { b.discards_left.max(0) as f64 } else { 0.0 };
    b.jokers.iter().filter(|jk| !jk.debuff && jk.key == "j_delayed_grat").map(|jk| jk.extra.n * unused).sum()
}

impl Board {
    /// What discarding `discarded` (in hand order) does to the board, and the money it pays
    /// (`discard_money`): state_events.lua `G.FUNCS.discard_cards_from_highlighted` runs the
    /// `pre_discard` context once, then the `discard` context for each card (card.lua
    /// `Card:calculate_joker`). Burnt Joker (copies too) levels the discarded cards' hand on
    /// the round's first discard; the rest change the joker itself, not copies: Green Joker
    /// loses `extra.discard_sub` Mult once a discard, Ramen ×`extra` a card (eaten once it
    /// would reach ×1), Castle gains `extra.chip_mod` Chips a card of the round's suit (not
    /// debuffed), Hit the Road ×`extra` a Jack (not debuffed), Yorick ×`extra.xmult` every
    /// `extra.discards` cards. Then a discard is used. Not modelled: Purple Seal's Tarot, and
    /// what destroying Trading Card's card changes beyond the deck's size (Caino, Glass Joker).
    pub fn discard(&mut self, discarded: &[Card]) -> f64 {
        let money = discard_money(self, discarded);
        if self.discards_used <= 0 && !discarded.is_empty() {
            let burnt = (0..self.jokers.len()).filter_map(|j| effective_joker(self, j)).filter(|jk| jk.key == "j_burnt").count() as i64;
            if burnt > 0 {
                let h = hand::detect(discarded, self.rule_flags()).hand as usize;
                self.levels[h] = self.levels[h].with_level(self.levels[h].level + burnt);
            }
        }
        if self.discards_used <= 0 && discarded.len() == 1 && self.jokers.iter().any(|jk| !jk.debuff && jk.key == "j_trading") {
            // the card is destroyed, not discarded
            self.playing_cards -= 1;
        }
        let smeared = self.has(Kind::Smeared);
        let castle = self.castle_suit;
        let mut eaten = vec![false; self.jokers.len()];
        for (j, jk) in self.jokers.iter_mut().enumerate().filter(|(_, jk)| !jk.debuff) {
            let x = jk.extra.clone();
            for (i, c) in discarded.iter().enumerate() {
                match jk.key.as_str() {
                    "j_ramen" if !eaten[j] => {
                        if jk.x_mult - x.n <= 1.0 {
                            eaten[j] = true;
                        } else {
                            jk.x_mult -= x.n;
                        }
                    }
                    "j_yorick" => {
                        if jk.yorick_discards <= 1.0 {
                            jk.yorick_discards = x.discards;
                            jk.x_mult += x.xmult;
                        } else {
                            jk.yorick_discards -= 1.0;
                        }
                    }
                    "j_castle" if castle.is_some_and(|s| !c.debuff && is_suit(c, s, false, false, smeared)) => {
                        jk.extra.chips += x.chip_mod;
                    }
                    "j_hit_the_road" if !c.debuff && card_id(c, i) == 11 => jk.x_mult += x.n,
                    _ => {}
                }
            }
            if jk.key == "j_green_joker" && !discarded.is_empty() {
                jk.mult = (jk.mult - jk.extra.discard_sub).max(0.0);
            }
        }
        let mut k = 0;
        self.jokers.retain(|_| {
            k += 1;
            !eaten[k - 1]
        });
        self.discards_used += 1;
        self.discards_left = (self.discards_left - 1).max(0);
        self.dollars += money;
        money
    }

    /// The board as round `blind` starts: what lasts only a round reset (state_events.lua
    /// `new_round`: the blind's own state, hands played this round, discards used; card.lua
    /// `end_of_round` context: Hit the Road back to ×1, unless debuffed). Not modelled: the
    /// round's targets the game draws anew (Castle's suit, Mail-In's rank, Idol, Ancient
    /// Joker: this round's are kept) and Campfire's reset after a boss.
    pub fn new_round(&mut self, blind: &str) {
        self.blind = BlindRules { key: blind.to_string(), ..Default::default() };
        for l in &mut self.levels {
            l.played_this_round = 0;
        }
        self.discards_used = 0;
        for jk in self.jokers.iter_mut().filter(|jk| !jk.debuff && jk.key == "j_hit_the_road") {
            jk.x_mult = 1.0;
        }
    }

    /// What a scored hand (`o`, from `score` on this board) leaves for the next one: the
    /// jokers' state, the hand's level and your money (`Outcome`), the hand counted as played
    /// (The Eye and The Mouth remember it), then the `after` context (card.lua
    /// `Card:calculate_joker`, not copies): Ice Cream loses `extra.chip_mod` Chips and Seltzer
    /// a use, each gone at 0.
    pub fn after_hand(&mut self, o: &Outcome) {
        for (jk, st) in self.jokers.iter_mut().zip(&o.jokers) {
            jk.mult = st.mult;
            jk.x_mult = st.x_mult;
            jk.extra.chips = st.extra_chips;
        }
        self.levels[o.hand as usize] = o.level;
        self.dollars = o.money;
        self.hands_played += 1;
        self.hands_left = (self.hands_left - 1).max(0);
        if self.blind.key == "bl_eye" {
            self.blind.eye_seen |= o.hand.bit();
        }
        if self.blind.key == "bl_mouth" && self.blind.mouth_only.is_none() {
            self.blind.mouth_only = Some(o.hand);
        }
        self.jokers.retain_mut(|jk| {
            if jk.debuff {
                return true;
            }
            match jk.kind {
                Kind::IceCream => {
                    jk.extra.chips -= jk.extra.chip_mod;
                    jk.extra.chips > 0.0
                }
                Kind::Seltzer => {
                    jk.extra.n -= 1.0;
                    jk.extra.n > 0.0
                }
                _ => true,
            }
        });
    }
}

/// Scores `played` with `held` staying in hand, both in the order that scores most (the
/// player arranges them: `Outcome::played_order`, `Outcome::held_order`).
pub fn score<R: Rolls + ?Sized>(b: &Board, played: &[Card], held: &[Card], rolls: &mut R, trace: bool) -> Outcome {
    let info = hand::detect(played, b.rule_flags());
    score_detected(b, played, held, info, rolls, trace)
}

/// Scores `played` (in play order) with `held` staying in hand (in hand order), as the game
/// scores a hand that was played in that arrangement: for checking against real scores.
pub fn score_as_played<R: Rolls + ?Sized>(b: &Board, played: &[Card], held: &[Card], rolls: &mut R, trace: bool) -> Outcome {
    let info = hand::detect(played, b.rule_flags());
    pass(b, played, held, info, rolls, trace, false)
}

/// `score` with the hand already detected (`hand::detect(played, b.rule_flags())`).
pub fn score_detected<R: Rolls + ?Sized>(
    b: &Board,
    played: &[Card],
    held: &[Card],
    info: HandInfo,
    rolls: &mut R,
    trace: bool,
) -> Outcome {
    // The order given (each part arranged where its cards' maps are exact), noting whether
    // any roll was asked for
    let mut watched = Asked { inner: rolls, asked: false };
    let mut given = pass(b, played, held, info, &mut watched, trace, true);
    let asked = watched.asked;
    let proposed = std::mem::take(&mut given.proposed);
    if proposed == (None, None) {
        return given;
    }
    // The rest is checked as played: the order is chosen before any roll, as a player
    // arranges, so on passes with every roll failing (the one just made when it asked for none)
    let mut base = if asked { pass(b, played, held, hand::detect(played, b.rule_flags()), &mut Unlucky, false, true) } else { Outcome { trace: vec![], ..given.clone() } };
    let (pp, ph) = if asked { std::mem::take(&mut base.proposed) } else { proposed };
    let (fp, fh) = (base.played_order.clone(), base.held_order.clone());
    // the proposal, and each half of it alone (the other part as it's arranged anyway): kept
    // when the whole hand scores more with no less money and the jokers left the same (what a
    // hand earns or grows isn't traded for points here)
    let mut tries = vec![(pp.clone(), ph.clone())];
    if pp.is_some() && ph.is_some() {
        tries.push((pp, None));
        tries.push((None, ph));
    }
    let arrange = |order: &Option<Vec<usize>>, cards: &[Card]| order.as_ref().map_or(cards.to_vec(), |o| o.iter().map(|&i| cards[i]).collect::<Vec<_>>());
    type Order = Option<Vec<usize>>;
    let mut best: Option<(Outcome, Order, Order)> = None;
    for (p, h) in tries {
        let (p, h) = (p.or_else(|| fp.clone()), h.or_else(|| fh.clone()));
        let (pl, hl) = (arrange(&p, played), arrange(&h, held));
        let o = pass(b, &pl, &hl, hand::detect(&pl, b.rule_flags()), &mut Unlucky, false, false);
        let bar = best.as_ref().map_or(base.score, |x| x.0.score);
        if o.score > bar * (1.0 + 1e-9) && o.dollars >= base.dollars && o.jokers == base.jokers {
            best = Some((o, p, h));
        }
    }
    let Some((no_luck, p, h)) = best else { return given };
    // the order chosen, with the real rolls (the same pass when no roll is asked for)
    let (pl, hl) = (arrange(&p, played), arrange(&h, held));
    let mut o = if asked || trace { pass(b, &pl, &hl, hand::detect(&pl, b.rule_flags()), rolls, trace, false) } else { no_luck };
    if let Some(p) = &p {
        o.scoring = o.scoring.iter().map(|&k| p[k]).collect();
    }
    o.played_order = p;
    o.held_order = h;
    o
}

/// Rolls that note whether any was asked for.
struct Asked<'r, R: Rolls + ?Sized> {
    inner: &'r mut R,
    asked: bool,
}

impl<R: Rolls + ?Sized> Rolls for Asked<'_, R> {
    fn chance(&mut self, p: f64) -> bool {
        self.asked = true;
        self.inner.chance(p)
    }
    fn range(&mut self, min: i64, max: i64) -> i64 {
        self.asked = true;
        self.inner.range(min, max)
    }
    fn unit(&mut self) -> f64 {
        self.asked = true;
        self.inner.unit()
    }
}

fn pass<R: Rolls + ?Sized>(b: &Board, played: &[Card], held: &[Card], info: HandInfo, rolls: &mut R, trace: bool, arrange: bool) -> Outcome {
    let flags = b.rule_flags();
    let mut level = b.levels[info.hand as usize];
    level.played += 1;
    level.played_this_round += 1;
    let p = Pass {
        b,
        rolls,
        flags,
        played: std::borrow::Cow::Borrowed(played),
        held,
        info,
        js: b.jokers.iter().map(|j| JokerState { mult: j.mult, x_mult: j.x_mult, extra_chips: j.extra.chips }).collect(),
        // ease_hands_played(-1) runs before evaluate_play
        hands_left: b.hands_left - 1,
        dollars: b.dollars,
        earned: 0.0,
        lucky_trigger: false,
        blind_triggered: false,
        level,
        trace: trace.then(Vec::new),
        held_used: false,
        arrange,
        played_order: None,
        held_order: None,
        in_cards: false,
        card_rolls: false,
        reads_place: false,
        proposed: (None, None),
    };
    p.run()
}

impl<R: Rolls + ?Sized> Pass<'_, R> {
    /// Records a trace step; `source` only runs when tracing.
    fn rec(&mut self, source: impl FnOnce(&Self) -> String, chips: f64, mult: f64) {
        if self.trace.is_some() {
            let s = source(self);
            if let Some(t) = &mut self.trace {
                t.push(Step { source: s, chips, mult });
            }
        }
    }

    /// The played cards' steps, worked out in the order of `info.scoring`.
    fn played_ops(&mut self, ops: &mut Ops) {
        for si in 0..self.info.scoring.len() {
            let ci = self.info.scoring[si];
            if self.played[ci].debuff {
                self.blind_triggered = true;
            } else {
                let mut reps = 1u32;
                if self.played[ci].seal == Some(Seal::Red) {
                    reps += 1;
                }
                for j in 0..self.b.jokers.len() {
                    if let Some(e) = self.calc(j, Ctx::PlayRepetition(ci), 0) {
                        reps += e.reps;
                    }
                }
                for r in 0..reps {
                    self.played_card_ops(ci, r > 0, ops);
                }
            }
            ops.ends.push(ops.ops.len());
        }
    }

    /// The held cards' steps, worked out in the order of `held`.
    fn held_ops(&mut self, ops: &mut Ops) {
        for hi in 0..self.held.len() {
            let mut reps = 1u32;
            let mut k = 0;
            while k < reps {
                let c = self.held[hi];
                let mut own_x = 1.0;
                if !c.debuff && c.enhancement == Some(Enhancement::Steel) {
                    own_x = 1.5;
                }
                // The card's own effect, then each joker's, in order (same as building the
                // game's effects list first and applying it after).
                if own_x != 1.0 {
                    self.push_op(ops, Op { chips: 0.0, mult: 0.0, x: own_x }, |_| Some(format!("{} held (steel)", c.label())));
                }
                let mut any_joker = false;
                for j in 0..self.b.jokers.len() {
                    if let Some(e) = self.calc(j, Ctx::HeldIndividual(hi), 0) {
                        any_joker = true;
                        self.earned += e.dollars;
                        if e.h_mult != 0.0 || e.x != 1.0 {
                            self.push_op(ops, Op { chips: 0.0, mult: e.h_mult, x: e.x }, |p| Some(format!("{} on held {}", p.joker_name(j), c.label())));
                        }
                    }
                }
                if own_x != 1.0 || any_joker {
                    self.held_used = true;
                }
                if k == 0 {
                    let any = own_x != 1.0 || any_joker;
                    if any && !c.debuff && c.seal == Some(Seal::Red) {
                        reps += 1;
                    }
                    for j in 0..self.b.jokers.len() {
                        if let Some(e) = self.calc(j, Ctx::HeldRepetition(any), 0) {
                            reps += e.reps;
                        }
                    }
                }
                k += 1;
            }
            ops.ends.push(ops.ops.len());
        }
    }

    /// The order a part's cards' maps say scores most (when `arrange`), and whether the maps
    /// are exact for it: no effect read where a card sits and no card asked for a roll (an
    /// order is chosen before any roll), so moving them changes only how their Mult adds up
    /// and the order can be applied here. Otherwise it's a proposal `score` checks as played.
    fn arrangement(&self, ops: &Ops, mult: f64, rolls_before: bool) -> (Option<Vec<usize>>, bool) {
        let order = if self.arrange { ops.best_order(mult) } else { None };
        let exact = !self.reads_place && self.card_rolls == rolls_before;
        (order, exact)
    }

    /// Adds a step to `ops`; `label` (the trace step after it) only runs when tracing.
    fn push_op(&self, ops: &mut Ops, op: Op, label: impl FnOnce(&Self) -> Option<String>) {
        ops.ops.push(op);
        if self.trace.is_some() {
            ops.labels.push(label(self));
        }
    }

    /// Applies every card's steps, in `order` (the order they were worked out in when `None`).
    fn apply(&mut self, ops: &Ops, order: Option<&[usize]>, chips: &mut f64, mult: &mut f64) {
        for k in 0..ops.ends.len() {
            for oi in ops.card(order.map_or(k, |o| o[k])) {
                let o = ops.ops[oi];
                *chips += o.chips;
                *mult += o.mult;
                *mult *= o.x;
                if let Some(Some(l)) = ops.labels.get(oi) {
                    let l = l.clone();
                    self.rec(|_| l, *chips, *mult);
                }
            }
        }
    }

    fn joker_name(&self, j: usize) -> String {
        crate::data::GameData::bundled().name(&self.b.jokers[j].key).to_string()
    }

    fn contains(&self, h: HandType) -> bool {
        self.info.contains(h)
    }

    fn outcome(self, chips: f64, mult: f64, debuffed_hand: bool) -> Outcome {
        Outcome {
            hand: self.info.hand,
            scoring: self.info.scoring,
            chips,
            mult,
            score: (chips * mult).floor(),
            dollars: self.earned,
            debuffed_hand,
            trace: self.trace.unwrap_or_default(),
            money: self.dollars + self.earned,
            jokers: self.js,
            level: self.level,
            held_used: self.held_used,
            played_order: self.played_order,
            held_order: self.held_order,
            proposed: self.proposed,
        }
    }

    /// `Blind:debuff_hand`; also applies The Arm's level-down and The Ox.
    fn blind_blocks_hand(&mut self) -> bool {
        let bl = &self.b.blind;
        if bl.disabled {
            return false;
        }
        let h = self.info.hand;
        let blocked = match bl.key.as_str() {
            "bl_psychic" => self.played.len() < 5,
            "bl_eye" => bl.eye_seen & h.bit() != 0,
            "bl_mouth" => bl.mouth_only.is_some_and(|m| m != h),
            _ => false,
        };
        if blocked {
            self.blind_triggered = true;
            return true;
        }
        if bl.key == "bl_arm" && self.level.level > 1 {
            self.blind_triggered = true;
            self.level = self.level.with_level(self.level.level - 1);
        }
        if bl.key == "bl_ox" && self.b.most_played == Some(h) {
            self.blind_triggered = true;
            self.dollars = 0.0;
        }
        false
    }

    fn run(mut self) -> Outcome {
        if self.b.blind.active("bl_tooth") {
            self.dollars -= self.played.len() as f64;
        }
        if self.blind_blocks_hand() {
            self.rec(|_| "boss blocks this hand".into(), 0.0, 0.0);
            return self.outcome(0.0, 0.0, true);
        }

        // context.before: scaling jokers update, Space Joker may level up the hand
        for j in 0..self.b.jokers.len() {
            if let Some(e) = self.calc(j, Ctx::Before, 0)
                && e.level_up
            {
                self.level = self.level.with_level(self.level.level + 1);
            }
        }

        let mut chips = self.level.chips;
        let mut mult = self.level.mult;
        if self.b.blind.active("bl_flint") {
            self.blind_triggered = true;
            mult = (mult * 0.5 + 0.5).floor().max(1.0);
            chips = (chips * 0.5 + 0.5).floor().max(0.0);
        }
        let hand_name = self.info.hand.name();
        self.rec(|p| format!("{hand_name} L{}", p.level.level), chips, mult);

        // The player arranges the cards (state_events.lua `G.FUNCS.evaluate_play`: the played
        // cards score in the order they sit in hand when played, the held ones in the order of
        // `G.hand.cards`, both as the player dragged them). Each card's effect, worked out in
        // the order given, is one map of Mult; the order those maps say scores most is the
        // proposal `score` checks as played (`Outcome::played_order`, `held_order`).

        // Played cards
        self.in_cards = true;
        let mut ops = Ops::take();
        let rolls_before = self.card_rolls;
        self.played_ops(&mut ops);
        let (order, exact) = self.arrangement(&ops, mult, rolls_before);
        self.apply(&ops, order.as_deref().filter(|_| exact), &mut chips, &mut mult);
        let order = order.map(|o| {
            let mut v: Vec<usize> = o.iter().map(|&si| self.info.scoring[si]).collect();
            v.extend((0..self.played.len()).filter(|i| !self.info.scoring.contains(i)));
            v
        });
        if exact { self.played_order = order } else { self.proposed.0 = order }
        ops.give_back();

        // Held in hand
        let mut ops = Ops::take();
        let rolls_before = self.card_rolls;
        self.reads_place = false;
        self.held_ops(&mut ops);
        let (order, exact) = self.arrangement(&ops, mult, rolls_before);
        self.apply(&ops, order.as_deref().filter(|_| exact), &mut chips, &mut mult);
        if exact { self.held_order = order } else { self.proposed.1 = order }
        ops.give_back();
        self.in_cards = false;

        // Jokers left to right, then consumables (Observatory)
        for j in 0..self.b.jokers.len() {
            let jk = &self.b.jokers[j];
            let ed = if jk.debuff { None } else { jk.edition };
            match ed {
                Some(Edition::Foil) => {
                    chips += 50.0;
                    self.rec(|p| format!("{} (foil)", p.joker_name(j)), chips, mult);
                }
                Some(Edition::Holo) => {
                    mult += 10.0;
                    self.rec(|p| format!("{} (holo)", p.joker_name(j)), chips, mult);
                }
                _ => {}
            }
            if let Some(e) = self.calc(j, Ctx::Main, 0) {
                mult += e.mult;
                chips += e.chips;
                mult *= e.x;
                self.rec(|p| p.joker_name(j), chips, mult);
            }
            for v in 0..self.b.jokers.len() {
                if let Some(e) = self.calc(v, Ctx::OtherJoker(j), 0) {
                    mult *= e.x;
                    self.rec(|p| format!("{} on {}", p.joker_name(v), p.joker_name(j)), chips, mult);
                }
            }
            if ed == Some(Edition::Polychrome) {
                mult *= 1.5;
                self.rec(|p| format!("{} (polychrome)", p.joker_name(j)), chips, mult);
            }
        }
        if self.b.observatory {
            for &p in &self.b.planets_held {
                if p == self.info.hand {
                    mult *= 1.5;
                    self.rec(|_| format!("Observatory: {} planet held", p.name()), chips, mult);
                }
            }
        }

        if self.b.plasma {
            let half = ((chips + mult) / 2.0).floor();
            chips = half;
            mult = half;
            self.rec(|_| "Plasma Deck balance".into(), chips, mult);
        }
        self.outcome(chips, mult, false)
    }

    /// One trigger of a played card: `eval_card` for the card, then every joker's
    /// individual effect, applied chips → mult → dollars → ×mult → edition.
    fn played_card_ops(&mut self, ci: usize, retrigger: bool, ops: &mut Ops) {
        let c = self.played[ci];
        let p = self.b.probability;
        // Card:get_chip_bonus / get_chip_mult / get_chip_x_mult / get_p_dollars
        let (bonus, enh_mult, enh_x) = match c.enhancement {
            Some(Enhancement::Stone) => (50.0 + c.perma_bonus, 0.0, 0.0),
            Some(Enhancement::Bonus) => (c.rank.chips() + 30.0 + c.perma_bonus, 0.0, 0.0),
            Some(Enhancement::Mult) => (c.rank.chips() + c.perma_bonus, 4.0, 0.0),
            Some(Enhancement::Glass) => (c.rank.chips() + c.perma_bonus, 0.0, 2.0),
            Some(Enhancement::Lucky) => {
                self.card_rolls = true;
                let m = if self.rolls.chance(p / 5.0) {
                    self.lucky_trigger = true;
                    20.0
                } else {
                    0.0
                };
                (c.rank.chips() + c.perma_bonus, m, 0.0)
            }
            _ => (c.rank.chips() + c.perma_bonus, 0.0, 0.0),
        };
        let mut dollars = 0.0;
        if c.seal == Some(Seal::Gold) {
            dollars += 3.0;
        }
        if c.enhancement == Some(Enhancement::Lucky) && self.rolls.chance(p / 15.0) {
            self.lucky_trigger = true;
            dollars += 20.0;
        }
        self.earned += dollars;

        // The card's own effect table, then each joker's individual effect as it comes
        // (the game collects them first, but none depends on the running chips/mult).
        let tag = if retrigger { " (retrigger)" } else { "" };
        let label = |_: &Self| Some(format!("{}{tag}", c.label()));
        let own = Op { chips: bonus, mult: enh_mult, x: if enh_x > 0.0 { enh_x } else { 1.0 } };
        let edition = match c.edition {
            Some(Edition::Foil) => Some(Op { chips: 50.0, mult: 0.0, x: 1.0 }),
            Some(Edition::Holo) => Some(Op { chips: 0.0, mult: 10.0, x: 1.0 }),
            Some(Edition::Polychrome) => Some(Op { chips: 0.0, mult: 0.0, x: 1.5 }),
            _ => None,
        };
        match edition {
            Some(e) => {
                self.push_op(ops, own, |_| None);
                self.push_op(ops, e, label);
            }
            None => self.push_op(ops, own, label),
        }
        for j in 0..self.b.jokers.len() {
            if let Some(e) = self.calc(j, Ctx::PlayIndividual(ci), 0) {
                self.earned += e.dollars;
                if e.chips != 0.0 || e.mult != 0.0 || e.x != 1.0 {
                    self.push_op(ops, Op { chips: e.chips, mult: e.mult, x: e.x }, |p| Some(format!("{} on {}", p.joker_name(j), c.label())));
                }
            }
        }
        self.lucky_trigger = false;
    }

    fn face(&self, c: &Card, idx: usize) -> bool {
        is_face(c, idx, self.flags.pareidolia, false)
    }

    fn suit(&self, c: &Card, s: Suit) -> bool {
        is_suit(c, s, false, false, self.flags.smeared)
    }

    fn chance(&mut self, odds: f64) -> bool {
        self.card_rolls |= self.in_cards && odds > 0.0;
        odds > 0.0 && self.rolls.chance(self.b.probability / odds)
    }

    /// `Card:calculate_joker(context)` for joker `j`. `bp` > 0 inside a Blueprint/Brainstorm copy.
    fn calc(&mut self, j: usize, ctx: Ctx, bp: usize) -> Option<Eff> {
        let n = self.b.jokers.len();
        let jk = &self.b.jokers[j];
        if jk.debuff {
            return None;
        }
        let target = match jk.kind {
            Kind::Blueprint => Some(j + 1),
            Kind::Brainstorm => Some(0),
            _ => None,
        };
        if let Some(t) = target {
            if t < n && t != j {
                if bp + 1 > n + 1 {
                    return None;
                }
                if let Some(e) = self.calc(t, ctx, bp + 1) {
                    return Some(e);
                }
            }
            return None;
        }
        let blueprint = bp > 0;
        let x = &jk.extra;
        let st = self.js[j];
        match ctx {
            Ctx::Before => self.before(j, blueprint),
            Ctx::PlayIndividual(ci) => {
                let c = self.played[ci];
                let id = card_id(&c, ci);
                match jk.kind {
                    Kind::Hiker => {
                        self.played.to_mut()[ci].perma_bonus += x.n;
                        eff()
                    }
                    Kind::LuckyCat if self.lucky_trigger && !blueprint => {
                        self.js[j].x_mult += x.n;
                        eff()
                    }
                    Kind::Wee if id == 2 && !blueprint => {
                        self.js[j].extra_chips += x.chip_mod;
                        eff()
                    }
                    Kind::Photograph => {
                        let mut faces = self.info.scoring.iter().copied().filter(|&i| self.face(&self.played[i], i));
                        let first_face = faces.next();
                        // which face card is first depends on the order only when there are two
                        self.reads_place |= faces.next().is_some();
                        (first_face == Some(ci)).then_some(Eff { x: x.n, ..NONE })
                    }
                    Kind::Idol => {
                        let (rid, s) = self.b.idol?;
                        (id == i32::from(rid) && self.suit(&c, s)).then_some(Eff { x: x.n, ..NONE })
                    }
                    Kind::ScaryFace if self.face(&c, ci) => Some(Eff { chips: x.n, ..NONE }),
                    Kind::Smiley if self.face(&c, ci) => Some(Eff { mult: x.n, ..NONE }),
                    Kind::GoldenTicket if c.enhancement == Some(Enhancement::Gold) => Some(Eff { dollars: x.n, ..NONE }),
                    Kind::Scholar if id == 14 => Some(Eff { chips: x.chips, mult: x.mult, ..NONE }),
                    Kind::WalkieTalkie if id == 10 || id == 4 => Some(Eff { chips: x.chips, mult: x.mult, ..NONE }),
                    Kind::Business if self.face(&c, ci) && self.chance(x.n) => Some(Eff { dollars: 2.0, ..NONE }),
                    Kind::Fibonacci if matches!(id, 2 | 3 | 5 | 8 | 14) => Some(Eff { mult: x.n, ..NONE }),
                    Kind::EvenSteven if (0..=10).contains(&id) && id % 2 == 0 => Some(Eff { mult: x.n, ..NONE }),
                    Kind::OddTodd if ((0..=10).contains(&id) && id % 2 == 1) || id == 14 => {
                        Some(Eff { chips: x.n, ..NONE })
                    }
                    Kind::SuitMult if x.suit.is_some_and(|s| self.suit(&c, s)) => Some(Eff { mult: x.s_mult, ..NONE }),
                    Kind::RoughGem if self.suit(&c, Suit::Diamonds) => Some(Eff { dollars: x.n, ..NONE }),
                    Kind::OnyxAgate if self.suit(&c, Suit::Clubs) => Some(Eff { mult: x.n, ..NONE }),
                    Kind::Arrowhead if self.suit(&c, Suit::Spades) => Some(Eff { chips: x.n, ..NONE }),
                    Kind::Bloodstone if self.suit(&c, Suit::Hearts) && self.chance(x.odds) => {
                        Some(Eff { x: x.xmult, ..NONE })
                    }
                    Kind::Ancient if self.b.ancient_suit.is_some_and(|s| self.suit(&c, s)) => Some(Eff { x: x.n, ..NONE }),
                    Kind::Triboulet if id == 12 || id == 13 => Some(Eff { x: x.n, ..NONE }),
                    _ => None,
                }
            }
            Ctx::HeldIndividual(hi) => {
                let c = self.held[hi];
                let id = card_id(&c, 1000 + hi);
                match jk.kind {
                    Kind::ShootTheMoon if id == 12 => Some(if c.debuff { NONE } else { Eff { h_mult: 13.0, ..NONE } }),
                    Kind::Baron if id == 13 => Some(if c.debuff { NONE } else { Eff { x: x.n, ..NONE } }),
                    Kind::ReservedParking if self.face(&c, 1000 + hi) && self.chance(x.odds) => {
                        Some(if c.debuff { NONE } else { Eff { dollars: x.dollars, ..NONE } })
                    }
                    Kind::RaisedFist => {
                        // lowest base id among non-Stone held cards; ties go to the last one (>=)
                        let mut best: Option<(usize, u8)> = None;
                        let mut tied = false;
                        for (i, h) in self.held.iter().enumerate() {
                            if h.enhancement != Some(Enhancement::Stone) && best.is_none_or(|(_, bid)| bid >= h.rank.0) {
                                tied = best.is_some_and(|(_, bid)| bid == h.rank.0);
                                best = Some((i, h.rank.0));
                            }
                        }
                        // which card it goes on depends on the order only when the lowest rank is tied
                        self.reads_place |= tied;
                        let (bi, _) = best?;
                        (bi == hi).then(|| {
                            if c.debuff { NONE } else { Eff { h_mult: 2.0 * self.held[bi].rank.chips(), ..NONE } }
                        })
                    }
                    _ => None,
                }
            }
            Ctx::PlayRepetition(ci) => {
                let c = self.played[ci];
                let id = card_id(&c, ci);
                let reps = match jk.kind {
                    Kind::SockAndBuskin if self.face(&c, ci) => x.n,
                    Kind::HangingChad => {
                        self.reads_place = true;
                        if self.info.scoring.first() != Some(&ci) {
                            return None;
                        }
                        x.n
                    }
                    Kind::Dusk if self.hands_left == 0 => x.n,
                    Kind::Seltzer => 1.0,
                    Kind::Hack if (2..=5).contains(&id) => x.n,
                    _ => return None,
                };
                Some(Eff { reps: reps as u32, ..NONE })
            }
            Ctx::HeldRepetition(any_effect) => {
                (jk.kind == Kind::Mime && any_effect).then_some(Eff { reps: x.n as u32, ..NONE })
            }
            Ctx::OtherJoker(o) => {
                let other = &self.b.jokers[o];
                (jk.kind == Kind::Baseball && other.rarity == 2 && o != j).then_some(Eff { x: x.n, ..NONE })
            }
            Ctx::Main => self.main(j, st),
        }
    }

    /// `context.before`
    fn before(&mut self, j: usize, blueprint: bool) -> Option<Eff> {
        let jk = &self.b.jokers[j];
        let x = jk.extra.clone();
        match jk.kind {
            Kind::Trousers if (self.contains(HandType::TwoPair) || self.contains(HandType::FullHouse)) && !blueprint => {
                self.js[j].mult += x.n;
                eff()
            }
            Kind::Space if self.chance(x.n) => Some(Eff { level_up: true, ..NONE }),
            Kind::Square if self.played.len() == 4 && !blueprint => {
                self.js[j].extra_chips += x.chip_mod;
                eff()
            }
            Kind::Runner if self.contains(HandType::Straight) && !blueprint => {
                self.js[j].extra_chips += x.chip_mod;
                eff()
            }
            Kind::MidasMask if !blueprint => {
                let mut any = false;
                for i in self.info.scoring.clone() {
                    if self.face(&self.played[i], i) {
                        self.played.to_mut()[i].enhancement = Some(Enhancement::Gold);
                        any = true;
                    }
                }
                any.then_some(NONE)
            }
            Kind::Vampire if !blueprint => {
                let mut count = 0.0;
                for i in self.info.scoring.clone() {
                    let c = &mut self.played.to_mut()[i];
                    if c.enhancement.is_some() && !c.debuff {
                        c.enhancement = None;
                        count += 1.0;
                    }
                }
                (count > 0.0).then(|| {
                    self.js[j].x_mult += x.n * count;
                    NONE
                })
            }
            Kind::ToDoList if jk.to_do_hand == Some(self.info.hand) => {
                self.earned += x.dollars;
                Some(Eff { dollars: x.dollars, ..NONE })
            }
            Kind::RideTheBus if !blueprint => {
                let faces = self.info.scoring.iter().any(|&i| self.face(&self.played[i], i));
                if faces {
                    self.js[j].mult = 0.0;
                } else {
                    self.js[j].mult += x.n;
                }
                None
            }
            Kind::Obelisk if !blueprint => {
                let this = self.level.played;
                let h = self.info.hand as usize;
                let reset = !self.b.levels.iter().enumerate().any(|(i, l)| i != h && l.played >= this && l.visible);
                if reset {
                    self.js[j].x_mult = 1.0;
                } else {
                    self.js[j].x_mult += x.n;
                }
                None
            }
            Kind::GreenJoker if !blueprint => {
                self.js[j].mult += x.hand_add;
                eff()
            }
            _ => None,
        }
    }

    /// `context.joker_main`: the game returns the first branch that applies.
    fn main(&mut self, j: usize, st: JokerState) -> Option<Eff> {
        let b = self.b;
        let jk = &b.jokers[j];
        let x = &jk.extra;
        let xm = |v: f64| Some(Eff { x: v, ..NONE });
        let m = |v: f64| Some(Eff { mult: v, ..NONE });
        let c = |v: f64| Some(Eff { chips: v, ..NONE });

        if jk.kind == Kind::Loyalty {
            let every = x.every;
            // G.GAME.hands_played is only incremented after evaluate_play
            let done = b.hands_played as f64 - jk.hands_played_at_create;
            let remaining = (every - 1.0 - done).rem_euclid(every + 1.0);
            if remaining == every {
                return xm(x.xmult);
            }
        }
        // Card:update keeps these x_mults live
        let x_mult = match jk.kind {
            Kind::Throwback => 1.0 + b.skips as f64 * x.n,
            Kind::Stencil => {
                let stencils = b.jokers.iter().filter(|o| o.kind == Kind::Stencil).count() as f64;
                (b.joker_slots - b.jokers.len() as i64) as f64 + stencils
            }
            _ => st.x_mult,
        };
        if jk.kind != Kind::SeeingDouble && x_mult > 1.0 && jk.typ.is_none_or(|t| self.contains(t)) {
            return xm(x_mult);
        }
        if jk.t_mult > 0.0 && jk.typ.is_some_and(|t| self.contains(t)) {
            return m(jk.t_mult);
        }
        if jk.t_chips > 0.0 && jk.typ.is_some_and(|t| self.contains(t)) {
            return c(jk.t_chips);
        }
        let money = self.dollars + self.earned;
        match jk.kind {
            Kind::Half if self.played.len() as f64 <= x.size => m(x.mult),
            Kind::Abstract => m(b.jokers.len() as f64 * x.n),
            Kind::Acrobat if self.hands_left == 0 => xm(x.n),
            Kind::MysticSummit if b.discards_left as f64 == x.d_remaining => m(x.mult),
            Kind::Misprint => {
                let v = self.rolls.range(x.min as i64, x.max as i64);
                m(v as f64)
            }
            Kind::Banner if b.discards_left > 0 => c(b.discards_left as f64 * x.n),
            Kind::Stuntman => c(x.chip_mod),
            Kind::Matador if self.blind_triggered => {
                self.earned += x.n;
                Some(Eff { dollars: x.n, ..NONE })
            }
            Kind::Supernova => m(self.level.played as f64),
            Kind::Ceremonial if st.mult > 0.0 => m(st.mult),
            Kind::FlowerPot if self.flower_pot() => xm(x.n),
            Kind::SeeingDouble if self.seeing_double() => xm(x.n),
            Kind::Wee => c(st.extra_chips),
            Kind::Castle if st.extra_chips > 0.0 => c(st.extra_chips),
            Kind::BlueJoker if b.deck_remaining > 0 => c(x.n * b.deck_remaining as f64),
            Kind::Erosion if b.starting_deck_size - b.playing_cards > 0 => {
                m(x.n * (b.starting_deck_size - b.playing_cards) as f64)
            }
            Kind::Square | Kind::Runner | Kind::IceCream => c(st.extra_chips),
            Kind::StoneJoker if b.stone_tally > 0 => c(x.n * b.stone_tally as f64),
            Kind::SteelJoker if b.steel_tally > 0 => xm(1.0 + x.n * b.steel_tally as f64),
            Kind::Bull if money > 0.0 => c(x.n * money.max(0.0)),
            Kind::DriversLicense if b.driver_tally >= 16 => xm(x.n),
            Kind::Blackboard => {
                self.held_used = true;
                let all_black = self.held.iter().all(|h| {
                    is_suit(h, Suit::Clubs, false, true, self.flags.smeared)
                        || is_suit(h, Suit::Spades, false, true, self.flags.smeared)
                });
                if all_black { xm(x.n) } else { None }
            }
            Kind::Stencil if b.joker_slots - (b.jokers.len() as i64) > 0 => xm(x_mult),
            Kind::Swashbuckler => {
                let v: f64 = b.jokers.iter().enumerate().filter(|(i, _)| *i != j).map(|(_, o)| o.sell_value).sum();
                if v > 0.0 { m(v) } else { None }
            }
            Kind::Joker => m(st.mult),
            Kind::Trousers | Kind::RideTheBus | Kind::Flash | Kind::Popcorn | Kind::GreenJoker | Kind::RedCard
                if st.mult > 0.0 =>
            {
                m(st.mult)
            }
            Kind::FortuneTeller if b.tarots_used > 0 => m(b.tarots_used as f64),
            Kind::GrosMichel => m(x.mult),
            Kind::Cavendish => xm(x.xmult),
            Kind::CardSharp if self.level.played_this_round > 1 => xm(x.xmult),
            Kind::Bootstraps if (money / x.dollars).floor() >= 1.0 => m(x.mult * (money / x.dollars).floor()),
            Kind::Caino if jk.caino_xmult > 1.0 => xm(jk.caino_xmult),
            _ => None,
        }
    }

    /// Flower Pot: a Diamond, Club, Heart and Spade among the scoring cards.
    fn flower_pot(&self) -> bool {
        let mut seen = [false; 4];
        let order = [Suit::Hearts, Suit::Diamonds, Suit::Spades, Suit::Clubs];
        let sc: Vec<Card> = self.info.scoring.iter().map(|&i| self.played[i]).collect();
        for c in sc.iter().filter(|c| c.enhancement != Some(Enhancement::Wild)) {
            if let Some(k) = order.iter().position(|&s| is_suit(c, s, true, false, self.flags.smeared) && !seen[order_idx(s)]) {
                seen[order_idx(order[k])] = true;
            }
        }
        for c in sc.iter().filter(|c| c.enhancement == Some(Enhancement::Wild)) {
            if let Some(k) = order.iter().position(|&s| self.suit(c, s) && !seen[order_idx(s)]) {
                seen[order_idx(order[k])] = true;
            }
        }
        seen.iter().all(|&s| s)
    }

    /// Seeing Double: a scoring Club and a scoring card of another suit.
    fn seeing_double(&self) -> bool {
        let mut n = [0u32; 4];
        let sc: Vec<Card> = self.info.scoring.iter().map(|&i| self.played[i]).collect();
        for c in sc.iter().filter(|c| c.enhancement != Some(Enhancement::Wild)) {
            for s in Suit::ALL {
                if self.suit(c, s) {
                    n[order_idx(s)] += 1;
                }
            }
        }
        let order = [Suit::Clubs, Suit::Diamonds, Suit::Spades, Suit::Hearts];
        for c in sc.iter().filter(|c| c.enhancement == Some(Enhancement::Wild)) {
            if let Some(&s) = order.iter().find(|&&s| self.suit(c, s) && n[order_idx(s)] == 0) {
                n[order_idx(s)] += 1;
            }
        }
        let clubs = n[order_idx(Suit::Clubs)] > 0;
        let other = n[order_idx(Suit::Hearts)] > 0 || n[order_idx(Suit::Diamonds)] > 0 || n[order_idx(Suit::Spades)] > 0;
        clubs && other
    }
}

fn order_idx(s: Suit) -> usize {
    match s {
        Suit::Spades => 0,
        Suit::Hearts => 1,
        Suit::Clubs => 2,
        Suit::Diamonds => 3,
    }
}
