//! The scoring pass: a port of `G.FUNCS.evaluate_play` (state_events.lua) and the
//! scoring contexts of `Card:calculate_joker` (card.lua). Comments name the game
//! code each block mirrors.

use serde::{Deserialize, Serialize};

use super::hand::{self, HandInfo, HandType, RuleFlags, card_id, is_face, is_suit};
use super::joker::{Joker, Kind};
use super::rng::Rolls;
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
    /// Worth (in $) of drawing a Blue Seal card at all this round, e.g. to use a held
    /// Cryptid on it (the round simulation counts it once).
    pub seal_seen_value: f64,
    /// Consumable slots free for the planets Blue Seal cards held at round end make.
    pub planet_slots: i64,
    /// What a round is worth beyond winning it, for the round simulation's own choices
    /// (none: it just plays to win).
    #[serde(skip)]
    pub goals: Option<crate::sim::RoundGoals>,
    pub mail_rank: Option<u8>,
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
            seal_seen_value: 0.0,
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
#[derive(Clone, Copy)]
struct JState {
    mult: f64,
    x_mult: f64,
    extra_chips: f64,
}

struct Pass<'a, R: Rolls + ?Sized> {
    b: &'a Board,
    rolls: &'a mut R,
    flags: RuleFlags,
    played: Vec<Card>,
    held: Vec<Card>,
    info: HandInfo,
    js: Vec<JState>,
    hands_left: i64,
    dollars: f64,
    earned: f64,
    lucky_trigger: bool,
    blind_triggered: bool,
    level: Level,
    trace: Option<Vec<Step>>,
}

/// Scores `played` (in play order) with `held` staying in hand.
/// Money a discard pays (card.lua `calculate_joker`, discard context): Mail-In Rebate $5 per
/// discarded card of the round's rank (not debuffed); Faceless Joker $5 when 3 or more of the
/// discarded cards are faces (Pareidolia: every card is; debuffed cards aren't).
pub fn discard_money(b: &Board, discarded: &[Card]) -> f64 {
    let count = |key: &str| b.jokers.iter().filter(|j| j.key == key && !j.debuff).count() as f64;
    let mut money = 0.0;
    if let Some(rank) = b.mail_rank {
        let n = discarded.iter().filter(|c| c.rank.0 == rank && c.enhancement != Some(crate::model::Enhancement::Stone) && !c.debuff).count();
        money += 5.0 * count("j_mail") * n as f64;
    }
    let pareidolia = b.has(Kind::Pareidolia);
    let faces = discarded.iter().filter(|c| !c.debuff && (pareidolia || (11..=13).contains(&c.rank.0))).count();
    if faces >= 3 {
        money += 5.0 * count("j_faceless");
    }
    money
}

pub fn score<R: Rolls + ?Sized>(b: &Board, played: &[Card], held: &[Card], rolls: &mut R, trace: bool) -> Outcome {
    let info = hand::detect(played, b.rule_flags());
    score_detected(b, played, held, info, rolls, trace)
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
    let flags = b.rule_flags();
    let mut level = b.levels[info.hand as usize];
    level.played += 1;
    level.played_this_round += 1;
    let p = Pass {
        b,
        rolls,
        flags,
        played: played.to_vec(),
        held: held.to_vec(),
        info,
        js: b.jokers.iter().map(|j| JState { mult: j.mult, x_mult: j.x_mult, extra_chips: j.extra.chips }).collect(),
        // ease_hands_played(-1) runs before evaluate_play
        hands_left: b.hands_left - 1,
        dollars: b.dollars,
        earned: 0.0,
        lucky_trigger: false,
        blind_triggered: false,
        level,
        trace: trace.then(Vec::new),
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

    fn joker_name(&self, j: usize) -> String {
        crate::data::GameData::bundled().name(&self.b.jokers[j].key).to_string()
    }

    fn contains(&self, h: HandType) -> bool {
        self.info.contains(h)
    }

    fn outcome(self, chips: f64, mult: f64, debuffed_hand: bool) -> Outcome {
        Outcome {
            hand: self.info.hand,
            scoring: self.info.scoring.clone(),
            chips,
            mult,
            score: (chips * mult).floor(),
            dollars: self.earned,
            debuffed_hand,
            trace: self.trace.unwrap_or_default(),
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

        // Played cards, in position order
        for si in 0..self.info.scoring.len() {
            let ci = self.info.scoring[si];
            if self.played[ci].debuff {
                self.blind_triggered = true;
                continue;
            }
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
                self.score_played_card(ci, r > 0, &mut chips, &mut mult);
            }
        }

        // Held in hand
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
                    mult *= own_x;
                    self.rec(|_| format!("{} held (steel)", c.label()), chips, mult);
                }
                let mut any_joker = false;
                for j in 0..self.b.jokers.len() {
                    if let Some(e) = self.calc(j, Ctx::HeldIndividual(hi), 0) {
                        any_joker = true;
                        self.earned += e.dollars;
                        mult += e.h_mult;
                        mult *= e.x;
                        if e.h_mult != 0.0 || e.x != 1.0 {
                            self.rec(|p| format!("{} on held {}", p.joker_name(j), c.label()), chips, mult);
                        }
                    }
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
        }

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
    fn score_played_card(&mut self, ci: usize, retrigger: bool, chips: &mut f64, mult: &mut f64) {
        let c = self.played[ci];
        let p = self.b.probability;
        // Card:get_chip_bonus / get_chip_mult / get_chip_x_mult / get_p_dollars
        let (bonus, enh_mult, enh_x) = match c.enhancement {
            Some(Enhancement::Stone) => (50.0 + c.perma_bonus, 0.0, 0.0),
            Some(Enhancement::Bonus) => (c.rank.chips() + 30.0 + c.perma_bonus, 0.0, 0.0),
            Some(Enhancement::Mult) => (c.rank.chips() + c.perma_bonus, 4.0, 0.0),
            Some(Enhancement::Glass) => (c.rank.chips() + c.perma_bonus, 0.0, 2.0),
            Some(Enhancement::Lucky) => {
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
        *chips += bonus;
        *mult += enh_mult;
        if enh_x > 0.0 {
            *mult *= enh_x;
        }
        match c.edition {
            Some(Edition::Foil) => *chips += 50.0,
            Some(Edition::Holo) => *mult += 10.0,
            Some(Edition::Polychrome) => *mult *= 1.5,
            _ => {}
        }
        let tag = if retrigger { " (retrigger)" } else { "" };
        self.rec(|_| format!("{}{tag}", c.label()), *chips, *mult);
        for j in 0..self.b.jokers.len() {
            if let Some(e) = self.calc(j, Ctx::PlayIndividual(ci), 0) {
                *chips += e.chips;
                *mult += e.mult;
                self.earned += e.dollars;
                *mult *= e.x;
                if e.chips != 0.0 || e.mult != 0.0 || e.x != 1.0 {
                    self.rec(|p| format!("{} on {}", p.joker_name(j), c.label()), *chips, *mult);
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
                        self.played[ci].perma_bonus += x.n;
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
                        let first_face = self.info.scoring.iter().copied().find(|&i| self.face(&self.played[i], i));
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
                        for (i, h) in self.held.iter().enumerate() {
                            if h.enhancement != Some(Enhancement::Stone) && best.is_none_or(|(_, bid)| bid >= h.rank.0) {
                                best = Some((i, h.rank.0));
                            }
                        }
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
                    Kind::HangingChad if self.info.scoring.first() == Some(&ci) => x.n,
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
                        self.played[i].enhancement = Some(Enhancement::Gold);
                        any = true;
                    }
                }
                any.then_some(NONE)
            }
            Kind::Vampire if !blueprint => {
                let mut count = 0.0;
                for i in self.info.scoring.clone() {
                    let c = &mut self.played[i];
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
    fn main(&mut self, j: usize, st: JState) -> Option<Eff> {
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
