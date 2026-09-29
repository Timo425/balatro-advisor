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
                xmult: f(e, "Xmult"),
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
            },
            _ => Extra::default(),
        };
        let x_mult = ability.get("x_mult").and_then(serde_json::Value::as_f64).unwrap_or(1.0);
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
        }
    }

    /// A fresh copy as it would come out of the shop (`Card:set_ability` defaults).
    pub fn from_config(key: &str, config: &serde_json::Value, rarity: u8) -> Joker {
        let mut ab = serde_json::json!({
            "mult": config.get("mult").cloned().unwrap_or(0.into()),
            "t_mult": config.get("t_mult").cloned().unwrap_or(0.into()),
            "t_chips": config.get("t_chips").cloned().unwrap_or(0.into()),
            "x_mult": config.get("Xmult").cloned().unwrap_or(1.into()),
            "type": config.get("type").cloned().unwrap_or("".into()),
        });
        if let Some(e) = config.get("extra") {
            ab["extra"] = e.clone();
        }
        Joker::from_ability(key, &ab, rarity)
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
}
