//! Jokers as the scoring engine sees them: a `Kind` (what code runs) plus the
//! numbers from their `ability` table (what the save says their counters are).

use serde::{Deserialize, Serialize};

use super::hand::HandType;
use crate::model::{Edition, Suit};

/// Jokers with their own branch in `Card:calculate_joker` for a scoring context,
/// or a rule change in hand detection. Everything else is `Other`: it still gets
/// the game's generic `x_mult` / `t_mult` / `t_chips` handling from its ability table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Joker,
    SuitMult,
    Stencil,
    FourFingers,
    Mime,
    Ceremonial,
    Banner,
    MysticSummit,
    Loyalty,
    Misprint,
    Dusk,
    RaisedFist,
    Fibonacci,
    SteelJoker,
    ScaryFace,
    Abstract,
    Hack,
    Pareidolia,
    GrosMichel,
    EvenSteven,
    OddTodd,
    Scholar,
    Business,
    Supernova,
    RideTheBus,
    Space,
    Blackboard,
    Runner,
    IceCream,
    Splash,
    BlueJoker,
    Hiker,
    GreenJoker,
    ToDoList,
    Cavendish,
    CardSharp,
    RedCard,
    Square,
    Vampire,
    Shortcut,
    Baron,
    Obelisk,
    MidasMask,
    Photograph,
    Erosion,
    ReservedParking,
    FortuneTeller,
    StoneJoker,
    LuckyCat,
    Baseball,
    Bull,
    Flash,
    Popcorn,
    Trousers,
    Ancient,
    WalkieTalkie,
    Seltzer,
    Castle,
    Smiley,
    Acrobat,
    SockAndBuskin,
    Swashbuckler,
    Smeared,
    Throwback,
    HangingChad,
    RoughGem,
    Bloodstone,
    Arrowhead,
    OnyxAgate,
    FlowerPot,
    Blueprint,
    Wee,
    Idol,
    SeeingDouble,
    Matador,
    Stuntman,
    Brainstorm,
    ShootTheMoon,
    DriversLicense,
    Bootstraps,
    Caino,
    Triboulet,
    GoldenTicket,
    Half,
    Other,
}

impl Kind {
    pub fn from_key(key: &str) -> Kind {
        use Kind::*;
        match key {
            "j_joker" => Joker,
            "j_greedy_joker" | "j_lusty_joker" | "j_wrathful_joker" | "j_gluttenous_joker" => SuitMult,
            "j_stencil" => Stencil,
            "j_four_fingers" => FourFingers,
            "j_mime" => Mime,
            "j_ceremonial" => Ceremonial,
            "j_banner" => Banner,
            "j_mystic_summit" => MysticSummit,
            "j_loyalty_card" => Loyalty,
            "j_misprint" => Misprint,
            "j_dusk" => Dusk,
            "j_raised_fist" => RaisedFist,
            "j_fibonacci" => Fibonacci,
            "j_steel_joker" => SteelJoker,
            "j_scary_face" => ScaryFace,
            "j_abstract" => Abstract,
            "j_hack" => Hack,
            "j_pareidolia" => Pareidolia,
            "j_gros_michel" => GrosMichel,
            "j_even_steven" => EvenSteven,
            "j_odd_todd" => OddTodd,
            "j_scholar" => Scholar,
            "j_business" => Business,
            "j_supernova" => Supernova,
            "j_ride_the_bus" => RideTheBus,
            "j_space" => Space,
            "j_blackboard" => Blackboard,
            "j_runner" => Runner,
            "j_ice_cream" => IceCream,
            "j_splash" => Splash,
            "j_blue_joker" => BlueJoker,
            "j_hiker" => Hiker,
            "j_green_joker" => GreenJoker,
            "j_todo_list" => ToDoList,
            "j_cavendish" => Cavendish,
            "j_card_sharp" => CardSharp,
            "j_red_card" => RedCard,
            "j_square" => Square,
            "j_vampire" => Vampire,
            "j_shortcut" => Shortcut,
            "j_baron" => Baron,
            "j_obelisk" => Obelisk,
            "j_midas_mask" => MidasMask,
            "j_photograph" => Photograph,
            "j_erosion" => Erosion,
            "j_reserved_parking" => ReservedParking,
            "j_fortune_teller" => FortuneTeller,
            "j_stone" => StoneJoker,
            "j_lucky_cat" => LuckyCat,
            "j_baseball" => Baseball,
            "j_bull" => Bull,
            "j_flash" => Flash,
            "j_popcorn" => Popcorn,
            "j_trousers" => Trousers,
            "j_ancient" => Ancient,
            "j_walkie_talkie" => WalkieTalkie,
            "j_selzer" => Seltzer,
            "j_castle" => Castle,
            "j_smiley" => Smiley,
            "j_acrobat" => Acrobat,
            "j_sock_and_buskin" => SockAndBuskin,
            "j_swashbuckler" => Swashbuckler,
            "j_smeared" => Smeared,
            "j_throwback" => Throwback,
            "j_hanging_chad" => HangingChad,
            "j_rough_gem" => RoughGem,
            "j_bloodstone" => Bloodstone,
            "j_arrowhead" => Arrowhead,
            "j_onyx_agate" => OnyxAgate,
            "j_flower_pot" => FlowerPot,
            "j_blueprint" => Blueprint,
            "j_wee" => Wee,
            "j_idol" => Idol,
            "j_seeing_double" => SeeingDouble,
            "j_matador" => Matador,
            "j_stuntman" => Stuntman,
            "j_brainstorm" => Brainstorm,
            "j_shoot_the_moon" => ShootTheMoon,
            "j_drivers_license" => DriversLicense,
            "j_bootstraps" => Bootstraps,
            "j_caino" => Caino,
            "j_triboulet" => Triboulet,
            "j_ticket" => GoldenTicket,
            "j_half" => Half,
            _ => Other,
        }
    }
}

/// The joker's `ability.extra`, which is a number for some jokers and a table for others.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Extra {
    /// `extra` itself when it is a number.
    pub n: f64,
    pub chips: f64,
    pub chip_mod: f64,
    pub mult: f64,
    pub xmult: f64,
    pub s_mult: f64,
    pub suit: Option<Suit>,
    pub odds: f64,
    pub size: f64,
    pub d_remaining: f64,
    pub dollars: f64,
    pub every: f64,
    pub hand_add: f64,
    pub min: f64,
    pub max: f64,
    /// Turtle Bean's current extra hand size (shrinks each round)
    #[serde(default)]
    pub h_size: f64,
    /// Green Joker's Mult lost per discard
    #[serde(default)]
    pub discard_sub: f64,
    /// Yorick: discards per ×Mult step
    #[serde(default)]
    pub discards: f64,
    /// Faceless Joker: face cards a discard needs to pay
    #[serde(default)]
    pub faces: f64,
    /// Rocket: what its payout rises by at each boss beaten
    #[serde(default)]
    pub increase: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Joker {
    pub key: String,
    pub kind: Kind,
    pub edition: Option<Edition>,
    pub debuff: bool,
    pub rarity: u8,
    pub sell_value: f64,
    pub mult: f64,
    pub x_mult: f64,
    pub t_mult: f64,
    pub t_chips: f64,
    /// `ability.type`: the hand a Type Mult / Type Chips / Duo-style joker wants.
    pub typ: Option<HandType>,
    pub extra: Extra,
    pub caino_xmult: f64,
    pub hands_played_at_create: f64,
    pub to_do_hand: Option<HandType>,
    /// Yorick: discards left until its next ×Mult step (`ability.yorick_discards`)
    #[serde(default)]
    pub yorick_discards: f64,
    /// Its place on your board as read from the save (1 = leftmost; 0: a joker that isn't
    /// yours, a stand-in or a shop card), kept through a round's steps: tells it apart from its
    /// copies when one is used up (`sim::RoundGoals::grown`)
    #[serde(skip)]
    pub id: u32,
    /// Eternal (`ability.eternal`): it can't be sold (`Board::sell_joker`'s callers leave it)
    #[serde(default)]
    pub eternal: bool,
}

fn f(v: &serde_json::Value, k: &str) -> f64 {
    v.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0)
}

impl Joker {
    /// From a save's `ability` table (or a center `config` turned into one by `from_config`).
    pub fn from_ability(key: &str, ability: &serde_json::Value, rarity: u8) -> Joker {
        let extra = match ability.get("extra") {
            Some(serde_json::Value::Number(n)) => Extra { n: n.as_f64().unwrap_or(0.0), ..Extra::default() },
            Some(e @ serde_json::Value::Object(_)) => Extra {
                n: 0.0,
                chips: f(e, "chips"),
                chip_mod: f(e, "chip_mod"),
                mult: f(e, "mult"),
                // Yorick's is lowercase
                xmult: if e.get("Xmult").is_some() { f(e, "Xmult") } else { f(e, "xmult") },
                s_mult: f(e, "s_mult"),
                suit: e.get("suit").and_then(|s| s.as_str()).and_then(Suit::from_name),
                odds: f(e, "odds"),
                size: f(e, "size"),
                d_remaining: f(e, "d_remaining"),
                dollars: f(e, "dollars"),
                every: f(e, "every"),
                hand_add: f(e, "hand_add"),
                min: f(e, "min"),
                max: f(e, "max"),
                h_size: f(e, "h_size"),
                discard_sub: f(e, "discard_sub"),
                discards: f(e, "discards"),
                faces: f(e, "faces"),
                increase: f(e, "increase"),
            },
            _ => Extra::default(),
        };
        let x_mult = ability.get("x_mult").and_then(serde_json::Value::as_f64).unwrap_or(1.0);
        // a fresh Yorick starts at `extra.discards` (card.lua `Card:set_ability`)
        let yorick_discards = ability.get("yorick_discards").and_then(serde_json::Value::as_f64).unwrap_or(extra.discards);
        Joker {
            key: key.to_string(),
            kind: Kind::from_key(key),
            edition: None,
            debuff: false,
            rarity,
            sell_value: 0.0,
            mult: f(ability, "mult"),
            x_mult,
            t_mult: f(ability, "t_mult"),
            t_chips: f(ability, "t_chips"),
            typ: ability.get("type").and_then(|t| t.as_str()).and_then(HandType::from_name),
            extra,
            caino_xmult: ability.get("caino_xmult").and_then(serde_json::Value::as_f64).unwrap_or(1.0),
            hands_played_at_create: f(ability, "hands_played_at_create"),
            to_do_hand: ability.get("to_do_poker_hand").and_then(|t| t.as_str()).and_then(HandType::from_name),
            yorick_discards,
            id: 0,
            eternal: ability.get("eternal").and_then(serde_json::Value::as_bool).unwrap_or(false),
        }
    }

    /// A fresh copy as it would come out of the shop (`Card:set_ability` defaults).
    pub fn from_config(key: &str, config: &serde_json::Value, rarity: u8) -> Joker {
        Joker::from_ability(key, &ability_from_config(config), rarity)
    }

    /// From the save, with edition/debuff/sell value filled in.
    pub fn from_save(j: &crate::save::JokerCard, data: &crate::data::GameData) -> Joker {
        let rarity = data.center(&j.key).and_then(|c| c.rarity).unwrap_or(0);
        let mut out = Joker::from_ability(&j.key, &j.ability, rarity);
        out.edition = j.edition.or(j.pending_tag_edition);
        out.debuff = j.debuff;
        out.sell_value = j.sell_value as f64;
        out
    }

    /// From a key with shop defaults (for candidates not in the save).
    pub fn from_key(key: &str, data: &crate::data::GameData) -> Option<Joker> {
        let c = data.center(key).filter(|c| c.set == "Joker")?;
        let mut j = Joker::from_config(key, &c.config, c.rarity.unwrap_or(0));
        // sell value = floor(cost / 2), at least 1 (Card:set_cost)
        j.sell_value = ((c.cost as f64) / 2.0).floor().max(1.0);
        Some(j)
    }

    /// The Mult one `ev` adds to this joker (card.lua `Card:calculate_joker`: Red Card in the
    /// `skipping_booster` context, Flash Card in `reroll_shop`, each by its `ability.extra`;
    /// both `not context.blueprint`, so copies don't grow, and a debuffed joker doesn't run).
    pub fn mult_from(&self, ev: RunEvent) -> f64 {
        if self.debuff {
            return 0.0;
        }
        match (self.kind, ev) {
            (Kind::RedCard, RunEvent::SkipPack) | (Kind::Flash, RunEvent::Reroll) => self.extra.n,
            _ => 0.0,
        }
    }

    /// The ×Mult one `ev` adds to this joker: Throwback scores ×`extra` for every blind skipped
    /// (card.lua: `1 + G.GAME.skips * extra`), through the board's count (`Board::after`).
    pub fn xmult_from(&self, ev: RunEvent) -> f64 {
        match (self.kind, ev) {
            (Kind::Throwback, RunEvent::SkipBlind) if !self.debuff => self.extra.n,
            _ => 0.0,
        }
    }

    /// The joker after `n` of `ev` (a fraction is a share of one)
    pub fn grow_from(&mut self, ev: RunEvent, n: f64) {
        self.mult += self.mult_from(ev) * n;
    }

    /// Whether the round's end puts its ×Mult back to ×1 (card.lua `end_of_round` context:
    /// Hit the Road, unless debuffed; `Board::new_round`)
    pub fn resets_at_round_end(&self) -> bool {
        !self.debuff && self.key == "j_hit_the_road"
    }

    /// What it pays at the end of a won round (card.lua `Card:calculate_dollar_bonus`, called on
    /// each joker itself by state_events.lua `G.FUNCS.evaluate_round`, so a copy pays nothing;
    /// none while debuffed): Golden Joker `extra`, Cloud 9 `extra` per 9 in the deck (the board's
    /// `nine_tally`), Rocket `extra.dollars`, Satellite `extra` per kind of planet used this run
    /// (`planets_used`), Delayed Gratification `extra` per discard left when none was used.
    pub fn round_end_dollars(&self, b: &super::Board) -> f64 {
        if self.debuff {
            return 0.0;
        }
        match self.key.as_str() {
            "j_golden" => self.extra.n,
            "j_cloud_9" => self.extra.n * b.nine_tally as f64,
            "j_rocket" => self.extra.dollars,
            "j_satellite" => self.extra.n * b.planets_used as f64,
            "j_delayed_grat" if b.discards_used == 0 => self.extra.n * b.discards_left.max(0) as f64,
            _ => 0.0,
        }
    }

    /// The joker after `n` more bosses beaten (a fraction is a share of one): what a boss's
    /// round end raises of its payout (card.lua `Card:calculate_joker`, `end_of_round` with
    /// `G.GAME.blind.boss`, not a copy): Rocket's `extra.dollars` by `extra.increase`. Nothing
    /// else a boss's end changes (Campfire's reset).
    pub fn payout_after_bosses(&mut self, n: f64) {
        if self.key == "j_rocket" && !self.debuff {
            self.extra.dollars += self.extra.increase * n;
        }
    }

    /// What a round's end adds to its own sell value (card.lua `Card:calculate_joker`,
    /// `end_of_round`, not a copy, not debuffed): Egg's `extra`. Gift Card's, which raises every
    /// other card's, isn't this.
    pub fn sell_value_a_round(&self) -> f64 {
        if self.debuff || self.key != "j_egg" {
            return 0.0;
        }
        self.extra.n
    }

    /// What it adds to the interest a round pays per $5 held (card.lua `Card:add_to_deck`:
    /// To the Moon raises `G.GAME.interest_amount` by `extra`; taken back while debuffed,
    /// `Card:set_debuff`)
    pub fn interest_added(&self) -> i64 {
        if self.debuff || self.key != "j_to_the_moon" {
            return 0;
        }
        self.extra.n as i64
    }

    /// Its Mult, ×Mult and Chips as they'll stand once the round is over (`resets_at_round_end`):
    /// what a round leaves of them (`sim::RoundGoals::jokers`)
    pub fn lasting(&self) -> [f64; 3] {
        [self.mult, if self.resets_at_round_end() { 1.0 } else { self.x_mult }, self.extra.chips]
    }

    /// The counter its ability keeps, apart from its three numbers (`lasting`), that a round's
    /// hands and discards move and a later round's scoring reads (card.lua `Card:calculate_joker`):
    /// Loyalty Card's hands since its ×Mult cycle began (`(G.GAME.hands_played −
    /// hands_played_at_create) % (every + 1)`, `hands_played`: the board's; the game's
    /// `loyalty_remaining` is `every − 1` less that, its ×Mult on the hand it reaches `every`,
    /// where this reaches `every` too), Yorick's discards
    /// left until its next ×Mult step (`yorick_discards`), Seltzer's hands left (`extra`). `None`
    /// for every other joker, and for one debuffed (Yorick's and Seltzer's don't move then;
    /// Loyalty Card's does, from the run's hands, but a debuffed joker isn't among the round's
    /// goals). What `sim::RoundGoals`
    /// counts in the blinds still ahead (`value::Ahead::counter`); a test plays every joker to
    /// check it.
    pub fn counter(&self, hands_played: i64) -> Option<f64> {
        if self.debuff {
            return None;
        }
        match self.kind {
            Kind::Loyalty => Some((hands_played as f64 - self.hands_played_at_create).rem_euclid(self.extra.every + 1.0)),
            Kind::Seltzer => Some(self.extra.n),
            _ if self.key == "j_yorick" => Some(self.yorick_discards),
            _ => None,
        }
    }

    /// Its counter (`counter`) set to `v`, the rest as it is
    pub fn set_counter(&mut self, hands_played: i64, v: f64) {
        match self.kind {
            Kind::Loyalty => self.hands_played_at_create = hands_played as f64 - v,
            Kind::Seltzer => self.extra.n = v,
            _ if self.key == "j_yorick" => self.yorick_discards = v,
            _ => {}
        }
    }

    /// Its counter (`counter`) after `hands` more hands played and `cards` more cards discarded,
    /// as the engine's steps move it (`Board::after_hand`: Loyalty Card one on, Seltzer one
    /// down, used up at none; `Board::discard_by`: Yorick one down a card, back to `extra.discards`
    /// from its last); `None` where it has none or it's used up
    pub fn counter_after(&self, hands_played: i64, hands: i64, cards: i64) -> Option<f64> {
        let now = self.counter(hands_played)?;
        match self.kind {
            Kind::Loyalty => Some((now + hands as f64).rem_euclid(self.extra.every + 1.0)),
            Kind::Seltzer => Some(now - hands as f64).filter(|&n| n > 0.0),
            _ => Some((0..cards).fold(now, |y, _| if y <= 1.0 { self.extra.discards } else { y - 1.0 })),
        }
    }
}

/// The jokers that act when they're sold in the shop (card.lua `Card:calculate_joker`,
/// `selling_self`: Diet Cola makes a Double Tag, Invisible Joker copies a joker; Luchador's
/// disables a boss blind in play, so in the shop it does nothing), and those that grow when any
/// card is sold (`selling_card`, button_callbacks.lua `G.FUNCS.sell_card`: Campfire). A debuffed
/// one does neither. Not modelled: an option that sells a joker says so where they're involved.
pub const ACTS_WHEN_SOLD: &[&str] = &["j_diet_cola", "j_invisible"];
pub const GROWS_FROM_SALES: &[&str] = &["j_campfire"];

/// The jokers whose money the engine pays on any round: while scoring (`score`: Business Card,
/// Rough Gem, To Do List, Golden Ticket, Reserved Parking), for a discard (`discard_money`: Mail-In
/// Rebate, Faceless Joker, Trading Card), at a won round's end (`Joker::round_end_dollars`:
/// Golden Joker, Cloud 9, Rocket, Satellite, Delayed Gratification) and besides a round's play
/// (`Joker::sell_value_a_round`: Egg; `Joker::interest_added`: To the Moon). A joker whose game
/// role is money and isn't here (Gift Card, Credit Card, and Matador: the engine pays it, but only
/// when a boss's rule triggers, and a joker's money is measured on plain blinds) is "not
/// modelled" where it's given up; a test checks every money joker is one or the other.
pub const PAYS_MONEY: &[&str] = &[
    "j_business",
    "j_rough_gem",
    "j_todo_list",
    "j_ticket",
    "j_reserved_parking",
    "j_mail",
    "j_faceless",
    "j_trading",
    "j_golden",
    "j_cloud_9",
    "j_rocket",
    "j_satellite",
    "j_delayed_grat",
    "j_egg",
    "j_to_the_moon",
];

/// The jokers a round changes that grow by taking something from the cards they grow on
/// (card.lua `Card:calculate_joker`, `before` context: Vampire strips the enhancement of each
/// scored card it grows from), so their growth over rounds depends on the deck those rounds
/// leave: rounds dealt anew from the same deck would let them grow on the same card every
/// round (`sim::grown_over` leaves them out).
pub const GROWS_BY_TAKING_CARDS: &[&str] = &["j_vampire"];

/// Whether rounds played one after another measure how joker `key` grows (`sim::grown_over`): a
/// joker a round changes (`CHANGE_IN_ROUND`) that doesn't grow by taking from its cards
/// (`GROWS_BY_TAKING_CARDS`)
pub fn grows_in_rounds(key: &str) -> bool {
    CHANGE_IN_ROUND.contains(&key) && !GROWS_BY_TAKING_CARDS.contains(&key)
}

/// The jokers whose own numbers (Mult, ×Mult, Chips) a hand played or a discard changes for
/// longer than the round, or uses up: the arms of card.lua `Card:calculate_joker` that change
/// `ability` in the `before`, `individual`, `after` and `discard` contexts, as the engine plays
/// them (`score`, `Board::after_hand`, `Board::discard_by`). Hit the Road isn't one (back to ×1
/// at the round's end). What a round's value counts (`sim::RoundGoals::jokers`); a test plays
/// every joker to check the list. Not modelled: what grows when a played card is destroyed
/// (card.lua's `cards_destroyed` and `remove_playing_cards` contexts: Glass Joker, Caino), as
/// the engine destroys none of them in a hand.
pub const CHANGE_IN_ROUND: &[&str] = &[
    "j_green_joker",
    "j_ride_the_bus",
    "j_runner",
    "j_square",
    "j_trousers",
    "j_wee",
    "j_lucky_cat",
    "j_vampire",
    "j_obelisk",
    "j_ice_cream",
    "j_selzer",
    "j_ramen",
    "j_castle",
    "j_yorick",
];

/// Things that happen between rounds that the board grows from: jokers that grow on them
/// (`Joker::mult_from`) and the game's counters that jokers score from
/// (`Board::after`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunEvent {
    /// A booster pack skipped (`G.FUNCS.skip_booster`, any kind of pack)
    SkipPack,
    /// The shop rerolled (`G.FUNCS.reroll_shop`)
    Reroll,
    /// A blind skipped (`G.FUNCS.skip_blind`: `G.GAME.skips` +1, which Throwback scores from)
    SkipBlind,
}

impl RunEvent {
    pub const ALL: [RunEvent; 3] = [RunEvent::SkipPack, RunEvent::Reroll, RunEvent::SkipBlind];
}

/// The `ability` table `Card:set_ability` builds from a center's config (card.lua).
pub fn ability_from_config(config: &serde_json::Value) -> serde_json::Value {
    let g = |k: &str, d: serde_json::Value| config.get(k).cloned().unwrap_or(d);
    let mut ab = serde_json::json!({
        "mult": g("mult", 0.into()),
        "h_mult": g("h_mult", 0.into()),
        "h_x_mult": g("h_x_mult", 0.into()),
        "h_dollars": g("h_dollars", 0.into()),
        "p_dollars": g("p_dollars", 0.into()),
        "t_mult": g("t_mult", 0.into()),
        "t_chips": g("t_chips", 0.into()),
        "x_mult": g("Xmult", 1.into()),
        "h_size": g("h_size", 0.into()),
        "d_size": g("d_size", 0.into()),
        "type": g("type", "".into()),
    });
    if let Some(e) = config.get("extra") {
        ab["extra"] = e.clone();
    }
    ab
}
