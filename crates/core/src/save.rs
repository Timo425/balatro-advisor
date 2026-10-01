//! `save.jkr` → `RunState`.
//!
//! Field meanings verified against a real 1.0.1o save and the game source
//! (`card.lua` `Card:save`, `blind.lua` `Blind:set_blind`, `misc_functions.lua`
//! `get_blind_amount`/`save_run`). The "what the snapshot can't show" notes and the
//! pending-edition-tag logic are adapted from balatro-agent's `tools/balatro_state.py`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::data::GameData;
use crate::lua::Value;
use crate::model::*;
use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    /// The run's seed (`GAME.pseudorandom.seed`): identifies a run.
    #[serde(default)]
    pub seed: String,
    /// The run has been won (`GAME.won`).
    #[serde(default)]
    pub won: bool,
    pub game_version: String,
    pub screen: Screen,
    pub stake: u8,
    pub deck: String,
    pub ante: i64,
    pub win_ante: i64,
    /// `GAME.modifiers.scaling` (blind size table) and the deck's `ante_scaling`.
    pub blind_scaling: i64,
    pub ante_scaling: f64,
    pub round: i64,
    pub dollars: f64,
    pub interest_amount: i64,
    pub interest_cap: i64,
    /// Cash per unused hand at cash-out (`modifiers.money_per_hand`, default 1).
    pub money_per_hand: f64,
    /// What a shop's first reroll costs (`GAME.base_reroll_cost`; vouchers lower it).
    pub base_reroll_cost: i64,
    pub skips: i64,
    /// Hands played this run (`GAME.hands_played`, Loyalty Card).
    pub hands_played: i64,
    /// Tarots used this run (Fortune Teller).
    pub tarots_used: i64,
    /// What The Fool would copy (`GAME.last_tarot_planet`).
    #[serde(default)]
    pub last_tarot_planet: Option<String>,
    /// Erosion compares the deck against this.
    pub starting_deck_size: i64,
    /// The Ox.
    pub most_played_hand: String,
    pub hands_left: i64,
    pub discards_left: i64,
    /// Hands and discards a fresh round starts with.
    pub round_hands: i64,
    pub round_discards: i64,
    pub hand_size: i64,
    pub joker_slots: i64,
    pub consumable_slots: i64,
    /// `G.GAME.probabilities.normal` (2 with Oops! All 6s).
    pub probability_normal: f64,
    pub jokers: Vec<JokerCard>,
    pub consumables: Vec<ItemCard>,
    pub hand: Vec<Card>,
    pub draw_pile: Vec<Card>,
    pub discard_pile: Vec<Card>,
    pub hand_levels: BTreeMap<String, HandLevel>,
    pub blinds: Vec<BlindSlot>,
    pub current_blind: Option<CurrentBlind>,
    pub shop: Option<Shop>,
    pub open_pack: Vec<ItemCard>,
    pub vouchers: Vec<String>,
    pub tags: Vec<String>,
    pub round_targets: RoundTargets,
    /// Jokers seen this run (`GAME.used_jokers`): the shop won't offer them again without Showman.
    pub used_jokers: Vec<String>,
    pub pool_flags: Vec<String>,
    pub banned_keys: Vec<String>,
    /// Times each boss has been drawn this run (`GAME.bosses_used`): rerolls pick among the least used.
    #[serde(default)]
    pub bosses_used: Vec<(String, i64)>,
    pub shop_rates: ShopRates,
    pub snapshot: Snapshot,
}

/// Weights the shop uses to pick what each card slot is (`create_card_for_shop`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopRates {
    pub joker: f64,
    pub tarot: f64,
    pub planet: f64,
    pub spectral: f64,
    pub playing_card: f64,
    /// Card slots per shop (`GAME.shop.joker_max`).
    pub slots: i64,
}

impl ShopRates {
    pub fn joker_share(&self) -> f64 {
        let total = self.joker + self.tarot + self.planet + self.spectral + self.playing_card;
        if total > 0.0 { self.joker / total } else { 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    SelectingHand,
    HandPlayed,
    DrawToHand,
    GameOver,
    Shop,
    PlayTarot,
    BlindSelect,
    RoundEval,
    TarotPack,
    PlanetPack,
    Menu,
    SpectralPack,
    StandardPack,
    BuffoonPack,
    NewRound,
    Other(i64),
}

impl Screen {
    /// `G.STATES` numbers from globals.lua.
    fn from_state(n: i64) -> Screen {
        use Screen::*;
        match n {
            1 => SelectingHand,
            2 => HandPlayed,
            3 => DrawToHand,
            4 => GameOver,
            5 => Shop,
            6 => PlayTarot,
            7 => BlindSelect,
            8 => RoundEval,
            9 => TarotPack,
            10 => PlanetPack,
            11 => Menu,
            15 => SpectralPack,
            17 => StandardPack,
            18 => BuffoonPack,
            19 => NewRound,
            n => Other(n),
        }
    }

    pub fn in_pack(self) -> bool {
        matches!(self, Screen::TarotPack | Screen::PlanetPack | Screen::SpectralPack | Screen::StandardPack | Screen::BuffoonPack)
    }

    pub fn in_blind(self) -> bool {
        matches!(self, Screen::SelectingHand | Screen::HandPlayed | Screen::DrawToHand)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JokerCard {
    pub key: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
    pub eternal: bool,
    /// Rounds left if perishable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perishable: Option<i64>,
    pub rental: bool,
    pub debuff: bool,
    pub cost: i64,
    pub sell_value: i64,
    /// The raw `ability` table: scaling counters live here (`mult`, `x_mult`, `extra`…).
    pub ability: serde_json::Value,
    /// Set on shop jokers that an edition tag will change right after this save.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_tag_edition: Option<Edition>,
}

/// Consumables, vouchers, packs and non-joker shop cards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemCard {
    pub key: String,
    pub name: String,
    pub set: String,
    pub cost: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
    /// For playing cards offered in packs / shop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandLevel {
    pub level: i64,
    pub chips: f64,
    pub mult: f64,
    /// Level 1 values and per-level gains (`s_chips`, `l_chips`, … in the save).
    pub s_chips: f64,
    pub s_mult: f64,
    pub l_chips: f64,
    pub l_mult: f64,
    pub played: i64,
    pub played_this_round: i64,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindSlot {
    pub slot: String,
    pub key: String,
    pub name: String,
    /// `Select`, `Upcoming`, `Current`, `Defeated`, `Skipped`.
    pub state: String,
    pub target: f64,
    /// Cash for beating it (0 for the Small Blind from Red Stake up).
    pub reward: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentBlind {
    pub key: String,
    pub name: String,
    pub target: f64,
    pub scored: f64,
    pub disabled: bool,
    /// The Eye: hand types already played this round.
    pub hands_seen: Vec<String>,
    /// The Mouth: the only hand type allowed this round, once one was played.
    pub only_hand: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shop {
    pub jokers: Vec<JokerCard>,
    /// Non-joker cards in the card row (tarots, planets, playing cards).
    pub other_cards: Vec<ItemCard>,
    pub boosters: Vec<ItemCard>,
    pub vouchers: Vec<ItemCard>,
    pub reroll_cost: i64,
}

/// Per-round targets that live in `GAME.current_round`, not on the joker.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoundTargets {
    pub ancient_suit: Option<Suit>,
    pub castle_suit: Option<Suit>,
    pub idol: Option<(Rank, Suit)>,
    pub mail_rank: Option<Rank>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub path: PathBuf,
    /// Read from the advisor-live mod's `live.jkr` (current to the half second).
    pub live: bool,
    pub age_secs: Option<u64>,
    /// What this snapshot cannot show (the game saves only at checkpoints).
    pub caveats: Vec<String>,
}

pub fn save_path(save_dir: &Path, profile: u8) -> PathBuf {
    save_dir.join(profile.to_string()).join("save.jkr")
}

/// `save.jkr`, or the advisor-live mod's `live.jkr` when it is at least as new.
/// A run only counts while `save.jkr` exists (the game deletes it when a run ends).
pub fn run_path(save_dir: &Path, profile: u8) -> PathBuf {
    let save = save_path(save_dir, profile);
    let live = save_dir.join(profile.to_string()).join("live.jkr");
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    // While the advisor-live mod is running (fresh heartbeat), its file is the current
    // state even when the game's own save is newer: the game saves the moment a pack
    // opens, before the pack's cards exist.
    let beat = save_dir.join(profile.to_string()).join("live.beat");
    let running = mtime(&beat).and_then(|t| std::time::SystemTime::now().duration_since(t).ok()).is_some_and(|d| d.as_secs() < 10);
    match (mtime(&save), mtime(&live)) {
        (_, Some(_)) if running => live,
        (Some(s), Some(l)) if l >= s => live,
        _ => save,
    }
}

pub fn load(path: &Path, data: &GameData) -> Result<RunState, Error> {
    if !path.is_file() {
        return Err(Error::NoRun(path.to_path_buf()));
    }
    let v = crate::jkr::read(path)?;
    let age = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs());
    from_value(&v, data, path, age)
}

fn num(v: &Value) -> f64 {
    v.num().unwrap_or(0.0)
}

fn int(v: &Value) -> i64 {
    v.int().unwrap_or(0)
}

fn center_key(c: &Value) -> String {
    c.at("save_fields.center").str().unwrap_or_default().to_string()
}

fn edition(c: &Value) -> Option<Edition> {
    c.at("edition.type").str().and_then(Edition::from_type)
}

pub(crate) fn playing_card(c: &Value) -> Option<Card> {
    let rank = Rank::from_value_name(c.at("base.value").str()?)?;
    let suit = Suit::from_name(c.at("base.suit").str()?)?;
    Some(Card {
        rank,
        suit,
        enhancement: Enhancement::from_key(&center_key(c)),
        edition: edition(c),
        seal: c.get("seal").str().and_then(Seal::from_name),
        perma_bonus: num(c.at("ability.perma_bonus")),
        debuff: c.get("debuff").truthy(),
        face_down: c.get("facing").str() == Some("back"),
    })
}

fn joker_card(c: &Value, data: &GameData) -> JokerCard {
    let key = center_key(c);
    let ab = c.get("ability");
    JokerCard {
        name: data.center(&key).map_or_else(|| ab.get("name").str().unwrap_or(&key).to_string(), |x| x.name.clone()),
        edition: edition(c),
        eternal: ab.get("eternal").truthy(),
        perishable: ab.get("perishable").truthy().then(|| int(ab.get("perish_tally"))),
        rental: ab.get("rental").truthy(),
        debuff: c.get("debuff").truthy(),
        cost: int(c.get("cost")),
        sell_value: int(c.get("sell_cost")),
        ability: ab.to_json(),
        pending_tag_edition: None,
        key,
    }
}

fn item_card(c: &Value, data: &GameData) -> ItemCard {
    let key = center_key(c);
    let set = c.at("ability.set").str().unwrap_or_default().to_string();
    let is_playing = matches!(set.as_str(), "Default" | "Enhanced");
    ItemCard {
        name: data.center(&key).map_or_else(
            || c.get("label").str().or(c.at("ability.name").str()).unwrap_or(&key).to_string(),
            |x| x.name.clone(),
        ),
        set,
        cost: int(c.get("cost")),
        edition: edition(c),
        card: if is_playing { playing_card(c) } else { None },
        key,
    }
}

fn area<'a>(g: &'a Value, name: &str) -> Vec<&'a Value> {
    g.at("cardAreas").get(name).get("cards").list()
}

/// `get_blind_amount(ante)` from misc_functions.lua. `scaling` is `GAME.modifiers.scaling`
/// (1 by default, 2 from Green Stake, 3 from Purple Stake).
pub fn blind_amount(ante: i64, scaling: i64) -> f64 {
    let amounts: [f64; 8] = match scaling {
        2 => [300., 900., 2600., 8000., 20000., 36000., 60000., 100000.],
        3 => [300., 1000., 3200., 9000., 25000., 60000., 110000., 200000.],
        _ => [300., 800., 2000., 5000., 11000., 20000., 35000., 50000.],
    };
    if ante < 1 {
        return 100.0;
    }
    if ante <= 8 {
        return amounts[(ante - 1) as usize];
    }
    let (k, a, b, c) = (0.75, amounts[7], 1.6, (ante - 8) as f64);
    let d = 1.0 + 0.2 * c;
    let amount = (a * (b + (k * c).powf(d)).powf(c)).floor();
    let unit = 10f64.powf((amount.log10() - 1.0).floor());
    amount - amount % unit
}

/// Keys of a `{key = true}` set table.
fn true_keys(v: &Value) -> Vec<String> {
    v.table().map_or_else(Vec::new, |t| {
        t.entries
            .iter()
            .filter(|(_, v)| v.truthy())
            .filter_map(|(k, _)| match k {
                crate::lua::Key::Str(s) => Some(s.clone()),
                crate::lua::Key::Int(_) => None,
            })
            .collect()
    })
}

fn suit_at(v: &Value) -> Option<Suit> {
    v.get("suit").str().and_then(Suit::from_name)
}

fn rank_at(v: &Value) -> Option<Rank> {
    v.get("rank").str().and_then(Rank::from_value_name)
}

pub fn from_value(g: &Value, data: &GameData, path: &Path, age_secs: Option<u64>) -> Result<RunState, Error> {
    let game = g.get("GAME");
    if game.is_nil() {
        return Err(Error::Format("save has no GAME table".into()));
    }
    let cr = game.get("current_round");
    let rr = game.get("round_resets");
    let screen = Screen::from_state(int(g.get("STATE")));
    let ante = int(rr.get("ante"));
    let scaling = game.at("modifiers.scaling").int().unwrap_or(1);
    let ante_scaling = game.at("starting_params.ante_scaling").num().unwrap_or(1.0);

    let blinds = ["Small", "Big", "Boss"]
        .iter()
        .filter_map(|slot| {
            let key = rr.at("blind_choices").get(slot).str()?.to_string();
            let info = data.blind(&key);
            Some(BlindSlot {
                slot: slot.to_string(),
                name: info.map_or_else(|| key.clone(), |b| b.name.clone()),
                target: blind_amount(ante, scaling) * info.map_or(1.0, |b| b.mult) * ante_scaling,
                state: rr.at("blind_states").get(slot).str().unwrap_or_default().to_string(),
                reward: if game.at("modifiers.no_blind_reward").get(slot).truthy() {
                    0
                } else {
                    info.map_or(0, |b| b.dollars)
                },
                skip_tag: rr.at("blind_tags").get(slot).str().map(str::to_string),
                key,
            })
        })
        .collect();

    let blind = g.get("BLIND");
    let current_blind = blind.get("name").str().filter(|n| !n.is_empty()).map(|name| CurrentBlind {
        key: blind.get("config_blind").str().unwrap_or_default().to_string(),
        name: name.to_string(),
        target: num(blind.get("chips")),
        scored: num(game.get("chips")),
        disabled: blind.get("disabled").truthy(),
        hands_seen: blind.get("hands").table().map_or_else(Vec::new, |t| {
            t.entries
                .iter()
                .filter(|(_, v)| v.truthy())
                .filter_map(|(k, _)| match k {
                    crate::lua::Key::Str(s) => Some(s.clone()),
                    crate::lua::Key::Int(_) => None,
                })
                .collect()
        }),
        only_hand: blind.get("only_hand").str().map(str::to_string),
    });

    let hand_levels = game
        .get("hands")
        .table()
        .map(|t| {
            t.entries
                .iter()
                .filter_map(|(k, h)| {
                    let crate::lua::Key::Str(name) = k else { return None };
                    Some((
                        name.clone(),
                        HandLevel {
                            level: int(h.get("level")),
                            chips: num(h.get("chips")),
                            mult: num(h.get("mult")),
                            s_chips: num(h.get("s_chips")),
                            s_mult: num(h.get("s_mult")),
                            l_chips: num(h.get("l_chips")),
                            l_mult: num(h.get("l_mult")),
                            played: int(h.get("played")),
                            played_this_round: int(h.get("played_this_round")),
                            visible: h.get("visible").truthy(),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let cards = |name: &str| area(g, name).into_iter().filter_map(playing_card).collect::<Vec<_>>();
    // Only a card in your hand can be "face down" in the boss sense: the draw pile is face
    // down because it's a deck.
    let unflip = |mut v: Vec<Card>| {
        for c in &mut v {
            c.face_down = false;
        }
        v
    };
    let draw_pile = unflip(cards("deck"));
    // Cards mid-play (saved during HAND_PLAYED) go back into the round's discard.
    let mut discard_pile = unflip(cards("discard"));
    discard_pile.extend(unflip(cards("play")));

    let mut tags: Vec<String> = g
        .get("tags")
        .list()
        .iter()
        .filter_map(|t| t.get("key").str().map(str::to_string))
        .collect();

    let shop = (screen == Screen::Shop || !area(g, "shop_jokers").is_empty()).then(|| {
        let mut jokers = Vec::new();
        let mut other_cards = Vec::new();
        for c in area(g, "shop_jokers") {
            if c.at("ability.set").str() == Some("Joker") {
                jokers.push(joker_card(c, data));
            } else {
                other_cards.push(item_card(c, data));
            }
        }
        if screen == Screen::Shop {
            apply_pending_edition_tags(&mut jokers, &mut tags);
        }
        Shop {
            jokers,
            other_cards,
            boosters: area(g, "shop_booster").into_iter().map(|c| item_card(c, data)).collect(),
            vouchers: area(g, "shop_vouchers").into_iter().map(|c| item_card(c, data)).collect(),
            reroll_cost: int(cr.get("reroll_cost")),
        }
    });

    let areas = g.get("cardAreas");
    // Steamodded saves (the bot bench) keep sizes in card_limits.total_slots instead
    let limit = |name: &str| {
        let c = areas.get(name).get("config");
        c.get("card_limit").int().or_else(|| c.at("card_limits.total_slots").int()).unwrap_or(0)
    };
    let state = RunState {
        seed: game.at("pseudorandom.seed").str().unwrap_or_default().to_string(),
        won: game.get("won").truthy(),
        game_version: g.get("VERSION").str().unwrap_or_default().to_string(),
        screen,
        stake: game.get("stake").int().unwrap_or(1) as u8,
        deck: g.at("BACK.name").str().unwrap_or_default().to_string(),
        ante,
        win_ante: int(game.get("win_ante")),
        blind_scaling: scaling,
        ante_scaling,
        round: int(game.get("round")),
        dollars: num(game.get("dollars")),
        interest_amount: int(game.get("interest_amount")),
        interest_cap: int(game.get("interest_cap")),
        money_per_hand: game.at("modifiers.money_per_hand").num().unwrap_or(1.0),
        base_reroll_cost: game.get("base_reroll_cost").int().unwrap_or(5),
        skips: int(game.get("skips")),
        hands_played: int(game.get("hands_played")),
        tarots_used: int(game.at("consumeable_usage_total.tarot")),
        last_tarot_planet: game.get("last_tarot_planet").str().filter(|k| *k != "c_fool").map(str::to_string),
        starting_deck_size: game.get("starting_deck_size").int().unwrap_or(52),
        most_played_hand: cr.get("most_played_poker_hand").str().unwrap_or_default().to_string(),
        hands_left: int(cr.get("hands_left")),
        discards_left: int(cr.get("discards_left")),
        round_hands: int(rr.get("hands")),
        round_discards: int(rr.get("discards")),
        hand_size: limit("hand"),
        joker_slots: limit("jokers"),
        consumable_slots: limit("consumeables"),
        probability_normal: game.at("probabilities.normal").num().unwrap_or(1.0),
        jokers: area(g, "jokers").into_iter().map(|c| joker_card(c, data)).collect(),
        consumables: area(g, "consumeables").into_iter().map(|c| item_card(c, data)).collect(),
        hand: cards("hand"),
        draw_pile,
        discard_pile,
        hand_levels,
        blinds,
        current_blind,
        shop,
        open_pack: area(g, "pack_cards").into_iter().map(|c| item_card(c, data)).collect(),
        vouchers: game.get("used_vouchers").table().map_or_else(Vec::new, |t| {
            t.entries
                .iter()
                .filter(|(_, v)| v.truthy())
                .filter_map(|(k, _)| match k {
                    crate::lua::Key::Str(s) => Some(s.clone()),
                    crate::lua::Key::Int(_) => None,
                })
                .collect()
        }),
        tags,
        round_targets: RoundTargets {
            ancient_suit: suit_at(cr.get("ancient_card")),
            castle_suit: suit_at(cr.get("castle_card")),
            idol: rank_at(cr.get("idol_card")).zip(suit_at(cr.get("idol_card"))),
            mail_rank: rank_at(cr.get("mail_card")),
        },
        used_jokers: true_keys(game.get("used_jokers")),
        pool_flags: true_keys(game.get("pool_flags")),
        banned_keys: true_keys(game.get("banned_keys")),
        bosses_used: game.get("bosses_used").table().map_or_else(Vec::new, |t| {
            t.entries
                .iter()
                .filter_map(|(k, v)| match k {
                    crate::lua::Key::Str(s) => Some((s.clone(), v.int().unwrap_or(0))),
                    crate::lua::Key::Int(_) => None,
                })
                .collect()
        }),
        shop_rates: ShopRates {
            joker: game.get("joker_rate").num().unwrap_or(20.0),
            tarot: game.get("tarot_rate").num().unwrap_or(4.0),
            planet: game.get("planet_rate").num().unwrap_or(4.0),
            spectral: num(game.get("spectral_rate")),
            playing_card: num(game.get("playing_card_rate")),
            slots: game.at("shop.joker_max").int().unwrap_or(2),
        },
        snapshot: {
            let live = path.file_name().is_some_and(|n| n == "live.jkr");
            Snapshot { path: path.to_path_buf(), live, age_secs, caveats: if live { vec![] } else { caveats(g, screen) } }
        },
    };
    Ok(state)
}

/// Edition tags turn shop jokers into Foil/Holo/Poly/Negative (and free) right *after*
/// the shop save, so the save still shows plain jokers and unused tags. Mirrors
/// `create_card_for_shop` → `store_joker_modify`: each edition-less joker takes the
/// first unused edition tag.
fn apply_pending_edition_tags(jokers: &mut [JokerCard], tags: &mut Vec<String>) {
    let edition_of = |t: &str| match t {
        "tag_negative" => Some(Edition::Negative),
        "tag_foil" => Some(Edition::Foil),
        "tag_holo" => Some(Edition::Holo),
        "tag_polychrome" => Some(Edition::Polychrome),
        _ => None,
    };
    for j in jokers.iter_mut().filter(|j| j.edition.is_none()) {
        let Some(i) = tags.iter().position(|t| edition_of(t).is_some()) else { break };
        let tag = tags.remove(i);
        j.pending_tag_edition = edition_of(&tag);
        j.cost = 0;
    }
}

fn caveats(g: &Value, screen: Screen) -> Vec<String> {
    let action = g.get("ACTION");
    if action.get("type").str() == Some("use_card") {
        return vec!["Saved the moment a pack was opened: its contents are not in the save.".into()];
    }
    match screen {
        Screen::Shop => vec![
            "Saved on entering the shop (or the last reroll). Anything bought, sold or used since then \
             is not reflected: shop cards may be gone, money and jokers may differ."
                .into(),
        ],
        s if s.in_blind() => vec![
            "Saved at the start of this hand or after the last discard. Consumables used since then \
             are not reflected."
                .into(),
        ],
        Screen::BlindSelect => vec!["Saved on reaching blind select. A pack opened from a skip tag is not in the save.".into()],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blind_amounts_match_game_tables() {
        assert_eq!(blind_amount(1, 1), 300.0);
        assert_eq!(blind_amount(8, 3), 200000.0);
        assert_eq!(blind_amount(0, 1), 100.0);
        // Ante 9 white: floor(50000*(1.6+0.75^1.2)) = 115405, cut to 2 significant digits.
        let a9 = blind_amount(9, 1);
        assert_eq!(a9, 110000.0);
    }

    fn synthetic_save() -> Value {
        crate::lua::parse(
            r#"return {["STATE"]=5,["VERSION"]="1.0.1o-FULL",["BACK"]={["name"]="Red Deck",},
            ["tags"]={[1]={["key"]="tag_foil",},[2]={["key"]="tag_double",},},
            ["BLIND"]={["name"]="",["chips"]=0,},
            ["GAME"]={["stake"]=8,["dollars"]=12,["win_ante"]=8,["round"]=3,["chips"]=0,
              ["modifiers"]={["scaling"]=3,},["starting_params"]={["ante_scaling"]=1,},
              ["probabilities"]={["normal"]=1,},["used_vouchers"]={["v_grabber"]=true,},
              ["round_resets"]={["ante"]=2,["blind_choices"]={["Small"]="bl_small",["Big"]="bl_big",["Boss"]="bl_club",},
                ["blind_states"]={["Small"]="Defeated",["Big"]="Select",["Boss"]="Upcoming",},["blind_tags"]={["Big"]="tag_foil",},},
              ["current_round"]={["hands_left"]=4,["discards_left"]=3,["reroll_cost"]=5,
                ["ancient_card"]={["suit"]="Hearts",},["idol_card"]={["rank"]="Ace",["suit"]="Spades",["id"]=14,},},
              ["hands"]={["Pair"]={["level"]=2,["chips"]=25,["mult"]=3,["played"]=4,["played_this_round"]=0,["visible"]=true,},},},
            ["cardAreas"]={
              ["jokers"]={["cards"]={[1]={["save_fields"]={["center"]="j_green_joker",},["ability"]={["name"]="Green Joker",["set"]="Joker",["mult"]=3,["eternal"]=true,},["cost"]=4,["sell_cost"]=2,["edition"]={["type"]="holo",["holo"]=true,},},},["config"]={["card_limit"]=5,},},
              ["hand"]={["cards"]={},["config"]={["card_limit"]=8,},},
              ["consumeables"]={["cards"]={},["config"]={["card_limit"]=2,},},
              ["deck"]={["cards"]={[1]={["save_fields"]={["center"]="m_glass",["card"]="H_K",},["base"]={["value"]="King",["suit"]="Hearts",},["ability"]={["perma_bonus"]=5,["set"]="Enhanced",},["seal"]="Red",},},},
              ["shop_jokers"]={["cards"]={[1]={["save_fields"]={["center"]="j_jolly",},["ability"]={["name"]="Jolly Joker",["set"]="Joker",},["cost"]=3,},},},
            },}"#,
        )
        .unwrap()
    }

    #[test]
    fn reads_synthetic_save() {
        let s = from_value(&synthetic_save(), GameData::bundled(), Path::new("x"), None).unwrap();
        assert_eq!(s.screen, Screen::Shop);
        assert_eq!(s.stake, 8);
        assert_eq!(s.jokers[0].key, "j_green_joker");
        assert_eq!(s.jokers[0].edition, Some(Edition::Holo));
        assert!(s.jokers[0].eternal);
        assert_eq!(s.jokers[0].ability["mult"], 3);
        let k = &s.draw_pile[0];
        assert_eq!((k.rank, k.suit, k.enhancement, k.seal), (Rank::KING, Suit::Hearts, Some(Enhancement::Glass), Some(Seal::Red)));
        assert_eq!(k.perma_bonus, 5.0);
        // Big blind on ante 2, purple+ scaling: 1000 * 1.5
        assert_eq!(s.blinds[1].target, 1500.0);
        assert_eq!(s.blinds[2].name, "The Club");
        assert_eq!(s.hand_levels["Pair"].level, 2);
        assert_eq!(s.round_targets.idol, Some((Rank::ACE, Suit::Spades)));
        // Foil tag lands on the shop joker; the Double tag stays.
        let shop = s.shop.unwrap();
        assert_eq!(shop.jokers[0].pending_tag_edition, Some(Edition::Foil));
        assert_eq!(shop.jokers[0].cost, 0);
        assert_eq!(s.tags, vec!["tag_double"]);
    }
}
