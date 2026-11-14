//! Which states a check runs on (CLAUDE.md step 4), by two settings; with both, a state must
//! pass both:
//! - `BAV_ONLY=a,b`: the states (or fixtures) whose name contains any of these;
//! - `BAV_HOLDS=x,y`: those that hold any of these: `face_down` (a face-down card in hand),
//!   `boss` (the blind being played is a boss), `hand:N` (N or more cards in hand), or a game key
//!   or its start (`j_blackboard`, `c_hermit`, `bl_wall`, `v_grabber`, `tag_double`, `p_arcana`):
//!   a joker or consumable held or on offer (the shop, an open pack), a voucher bought or in
//!   the shop, a pack in the shop, the blind being played or one of the ante's, a tag held or
//!   one of the ante's blinds offers for a skip.
//!   `face_down` and `boss` read the screen alone, while the quick pass also plays the round's
//!   later draws and the ante's blinds ahead: those a blind's key finds (`bl_wheel` turns drawn
//!   cards face down; CLAUDE.md step 4 says which kinds a change takes).
//!   Not there to be found: an open pack's own key (the state holds only its cards), and on a
//!   shop screen the tags the shop already applied to its jokers (`save.rs`
//!   `apply_pending_store_tags`, `apply_pending_edition_tags`: a free joker, a joker's
//!   `pending_tag_edition`); name such states with `BAV_ONLY`.
//!
//! A term that's none of these, a `BAV_ONLY` name or a game key that none of the states has, or
//! settings that keep none of them, panic: a typo mustn't leave a check that quietly ran on
//! less than it was pointed at (a key held by none may also be one the sample doesn't reach:
//! `BAV_SAMPLE=0` takes the whole history). `face_down`, `boss` and `hand:N` may keep none.
//!
//! Reads the state as `balatro-advisor state --json` writes it (`RunState`'s fields, long
//! stable) with `serde_json` alone, so it can be copied into an older base's worktree with
//! `history_picks.rs`.
#![allow(dead_code)]

use serde_json::Value;

/// One kind of state `BAV_HOLDS` asks for
#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    FaceDown,
    Boss,
    /// at least this many cards in hand
    Hand(usize),
    /// a game key, or its start
    Key(String),
}

/// The game keys' prefixes a key term may start with (game.lua's prototypes: jokers,
/// consumables, vouchers, blinds, tags, booster packs)
const PREFIXES: [&str; 6] = ["j_", "c_", "v_", "bl_", "tag_", "p_"];

/// The fields of a state that hold what a key term looks for: not the run's history
/// (`used_jokers`, `bosses_used`, `banned_keys`, `last_tarot_planet`), which a state doesn't hold
const HELD: [&str; 8] = ["jokers", "consumables", "shop", "open_pack", "vouchers", "current_blind", "blinds", "tags"];

/// `BAV_HOLDS`'s value read as terms (comma-separated); panics on one that's none of the kinds
pub fn parse(spec: &str) -> Vec<Term> {
    spec.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| match t {
            "face_down" => Term::FaceDown,
            "boss" => Term::Boss,
            _ if t.starts_with("hand:") => Term::Hand(t["hand:".len()..].parse().unwrap_or_else(|_| panic!("BAV_HOLDS: {t}: hand:N takes a number of cards"))),
            _ if PREFIXES.iter().any(|p| t.starts_with(p)) => Term::Key(t.to_string()),
            _ => panic!("BAV_HOLDS: {t} is none of face_down, boss, hand:N or a game key (starting {})", PREFIXES.join(", ")),
        })
        .collect()
}

/// Every string in `v`, at any depth
fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

impl Term {
    /// Whether `state` (a `RunState` as JSON) holds this
    pub fn holds(&self, state: &Value) -> bool {
        let hand = state["hand"].as_array().map_or(&[][..], Vec::as_slice);
        match self {
            Term::FaceDown => hand.iter().any(|c| c["face_down"] == true),
            Term::Boss => state["current_blind"]["key"].as_str().is_some_and(|k| k != "bl_small" && k != "bl_big"),
            Term::Hand(n) => hand.len() >= *n,
            Term::Key(k) => {
                let mut s = vec![];
                HELD.iter().for_each(|f| strings(&state[*f], &mut s));
                s.iter().any(|s| s.starts_with(k.as_str()))
            }
        }
    }
}

/// A setting's comma-separated names, or none when it's unset or names nothing
fn names(var: &str) -> Option<Vec<String>> {
    let v: Vec<String> = std::env::var(var).ok()?.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    (!v.is_empty()).then_some(v)
}

/// `items` as `BAV_ONLY` and `BAV_HOLDS` leave them, in their order: `name` is what `BAV_ONLY`
/// looks in, `state` the state as JSON (read once per item, only when `BAV_HOLDS` is set; an
/// item whose state can't be read is kept, so a check still shows it failing). Prints how many
/// were kept.
pub fn keep<T>(items: Vec<T>, name: impl Fn(&T) -> String, state: impl Fn(&T) -> Option<Value>) -> Vec<T> {
    let only = names("BAV_ONLY");
    let spec = names("BAV_HOLDS");
    let holds = spec.as_ref().map(|t| parse(&t.join(",")));
    if only.is_none() && holds.is_none() || items.is_empty() {
        return items;
    }
    let n = items.len();
    // each item's marks: the names its name contains, and the terms its state holds (none when
    // it can't be read); the states aren't kept, so a filter over the whole history stays small
    let marks: Vec<(Vec<bool>, Option<Vec<bool>>)> = items
        .iter()
        .map(|x| {
            let nm = name(x);
            let by_name = only.iter().flatten().map(|o| nm.contains(o.as_str())).collect();
            (by_name, holds.as_ref().and_then(|h| state(x).map(|s| h.iter().map(|t| t.holds(&s)).collect())))
        })
        .collect();
    for (i, o) in only.iter().flatten().enumerate() {
        assert!(marks.iter().any(|m| m.0[i]), "BAV_ONLY: {o} is in the name of none of the {n}: mistyped?");
    }
    for (i, t) in holds.iter().flatten().enumerate() {
        if let Term::Key(k) = t {
            assert!(marks.iter().any(|m| m.1.as_ref().is_some_and(|h| h[i])), "BAV_HOLDS: none of the {n} has {k}: mistyped, or not in the sample (`BAV_SAMPLE=0` takes the whole history)?");
        }
    }
    let kept: Vec<T> = items
        .into_iter()
        .zip(&marks)
        .filter(|(_, (by_name, by_state))| (only.is_none() || by_name.iter().any(|b| *b)) && (holds.is_none() || by_state.as_ref().is_none_or(|h| h.iter().any(|b| *b))))
        .map(|(x, _)| x)
        .collect();
    let set = [("BAV_ONLY", &only), ("BAV_HOLDS", &spec)].iter().filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={}", v.join(",")))).collect::<Vec<_>>().join(" ");
    eprintln!("{set}: {} of {n} kept", kept.len());
    assert!(!kept.is_empty(), "{set} keeps none of the {n}");
    kept
}
