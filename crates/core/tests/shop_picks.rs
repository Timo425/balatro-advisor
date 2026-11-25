//! The shop ranking's top options on a list of saved shop states, each with the option the shop
//! judge plays for it (`shop_playon.rs`, gap 17 in docs/design.md: "How the picks were judged"),
//! for judging the top picks a change moved (`shop_playon.rs` `shop_judge`). Uses only
//! long-stable API, so it can be copied into a `git worktree` of an older base commit (with
//! `common/stamp.rs` and `common/source_hash.rs`) and run there:
//! `BAV_SHOP_PICKS=/tmp/before cargo test --release --test shop_picks -- --ignored shop_picks
//! --nocapture`. The states: `BAV_SHOP_STATES` (default the 144 shops of
//! `tests/fixtures/private/shop_judge_states.txt`), one path a line; a path that isn't on this
//! machine is looked up by its last two parts (`<seed>/<state>.jkr`) in the saved history
//! (`BAV_HISTORY`, else the live page's folder), so a list made on another machine runs here.
//! `BAV_ONLY=a,b` keeps the states whose path contains any of these. Each state is analysed as
//! `balatro-advisor analyze --file` does (`advise::Options::default()`, the full analysis),
//! about 10-60 s a shop. `BAV_SHOP_TOP` options a state (default 3).
//!
//! The judge's option is read from the option's own fields and the shop it's in (its kind, key,
//! price, the joker it sells; a joker's edition from the shop's card of that key; a pack's
//! size and picks from the game's data), never from its label: `joker:<key>[:<edition>]`,
//! `sell:<key>[:<place>]` (its place in your row where you hold it twice), `reroll`, `leave`,
//! `pack:<jokers>:<price>[:<picks>]` (Buffoon), `celestial:<planets>:<price>[:<picks>]`,
//! `arcana:` / `spectral:<cards>:<price>[:<picks>]`,
//! `planet:<hand>:<price>`, `tarot:<key>:<price>`, `voucher:<key>:<price>` (Seed Money, Money
//! Tree). What the judge can't play is written `-` with the reason (a Standard pack, a card or
//! joker picked from an open pack, a voucher but those two).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use balatro_advisor::model::Edition;
use balatro_advisor::{advise, calibration, data::GameData, save};

#[path = "common/stamp.rs"]
pub mod stamp;

/// The list `BAV_SHOP_STATES` names, else the fixtures' 144 judge shops
pub fn list() -> PathBuf {
    std::env::var_os("BAV_SHOP_STATES").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/private/shop_judge_states.txt"))
}

/// A listed state's name (`<seed>/<state>.jkr`) and where it is on this machine (none: not found)
pub fn resolve(line: &str) -> (String, Option<PathBuf>) {
    let p = PathBuf::from(line.trim());
    let parts: Vec<String> = p.iter().map(|s| s.to_string_lossy().to_string()).collect();
    let name = parts[parts.len().saturating_sub(2)..].join("/");
    if p.exists() {
        return (name, Some(p));
    }
    let dir = std::env::var_os("BAV_HISTORY").map(PathBuf::from).or_else(|| calibration::default_dir().map(|d| d.join("history")));
    (name.clone(), dir.map(|d| d.join(&name)).filter(|q| q.exists()))
}

/// The listed states `BAV_ONLY` keeps, by name, in the list's order
pub fn states() -> Vec<(String, Option<PathBuf>)> {
    let only: Vec<String> = std::env::var("BAV_ONLY").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    let text = std::fs::read_to_string(list()).unwrap_or_else(|e| panic!("{}: {e}", list().display()));
    text.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).filter(|l| only.is_empty() || only.iter().any(|o| l.contains(o.as_str()))).map(resolve).collect()
}

/// A pack's cards and picks (the game's `config.extra`, `config.choose`)
fn pack_size(data: &GameData, key: &str) -> Option<(i64, i64)> {
    let c = data.center(key)?;
    Some((c.config.get("extra")?.as_i64()?, c.config.get("choose").and_then(|v| v.as_i64()).unwrap_or(1)))
}

/// The option the shop judge plays for `o`, or why it can't
pub fn judge_option(o: &advise::ShopOption, run: &save::RunState, data: &GameData) -> Result<String, String> {
    let key = o.key.as_deref();
    let in_pack = |k: &str| run.open_pack.iter().any(|c| c.key == k);
    match (o.kind.as_str(), key) {
        ("leave", _) => Ok("leave".into()),
        ("reroll", _) => Ok("reroll".into()),
        ("sell", _) => {
            let k = o.sell_key.as_ref().ok_or("a sale without its joker's key")?;
            // the place in your row (from 1) where you hold that joker more than once
            let twice = run.jokers.iter().filter(|j| &j.key == k).count() > 1;
            Ok(match o.sell_index.filter(|_| twice) {
                Some(i) => format!("sell:{k}:{}", i + 1),
                None => format!("sell:{k}"),
            })
        }
        ("joker", Some(k)) if in_pack(k) => Err("a joker picked from an open pack".into()),
        ("joker", Some(k)) => {
            let shop = run.shop.as_ref().map(|s| s.jokers.as_slice()).unwrap_or_default();
            // the shop's card of that key, the one at the option's price first
            let card = shop.iter().filter(|c| c.key == k).min_by_key(|c| (c.cost != o.cost) as u8);
            let ed = card.and_then(|c| c.edition).map(|e| match e {
                Edition::Foil => ":foil",
                Edition::Holo => ":holo",
                Edition::Polychrome => ":poly",
                Edition::Negative => ":negative",
            });
            Ok(format!("joker:{k}{}", ed.unwrap_or("")))
        }
        ("planet", Some(k)) => {
            let hand = data.center(k).and_then(|c| c.config.get("hand_type")?.as_str().map(str::to_string)).ok_or_else(|| format!("{k}: no hand"))?;
            Ok(format!("planet:{hand}:{}", o.cost))
        }
        ("tarot", Some(k)) => Ok(format!("tarot:{k}:{}", o.cost)),
        ("voucher", Some(k @ ("v_seed_money" | "v_money_tree"))) => Ok(format!("voucher:{k}:{}", o.cost)),
        ("voucher", Some(k)) => Err(format!("{k}: a voucher the judge doesn't play")),
        ("pack", Some(k)) => {
            let (n, picks) = pack_size(data, k).ok_or_else(|| format!("{k}: no pack size"))?;
            let more = if picks > 1 { format!(":{picks}") } else { String::new() };
            if k.starts_with("p_buffoon") {
                Ok(format!("pack:{n}:{}{more}", o.cost))
            } else if k.starts_with("p_celestial") {
                Ok(format!("celestial:{n}:{}{more}", o.cost))
            } else if k.starts_with("p_arcana") {
                Ok(format!("arcana:{n}:{}{more}", o.cost))
            } else if k.starts_with("p_spectral") {
                Ok(format!("spectral:{n}:{}{more}", o.cost))
            } else {
                Err(format!("{k}: a pack the judge doesn't play"))
            }
        }
        ("card", _) => Err("a playing card picked from an open pack".into()),
        (kind, _) => Err(format!("a {kind} option the judge doesn't play")),
    }
}

/// A `picks.tsv` row: the state, the option's place, the judge's option (`-`: none), the label,
/// why it can't be judged
pub type Row = (String, usize, String, String, String);

/// `$dir/picks.tsv`'s rows
pub fn read(dir: &Path) -> Vec<Row> {
    let text = std::fs::read_to_string(dir.join("picks.tsv")).unwrap_or_else(|e| panic!("{}/picks.tsv: {e}", dir.display()));
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some((f.first()?.to_string(), f.get(1)?.parse().ok()?, f.get(2)?.to_string(), f.get(3).unwrap_or(&"").to_string(), f.get(4).unwrap_or(&"").to_string()))
        })
        .collect()
}

/// Writes each listed shop's top options to `$BAV_SHOP_PICKS/picks.tsv` (state, place from 1,
/// the judge's option or `-`, label, why not judged; a state that won't load or isn't found
/// gets a place-0 row saying so) and each state's time to `times.tsv`. An earlier run's file is
/// moved aside first (`picks.previous.tsv`).
#[test]
#[ignore]
fn shop_picks() {
    let Some(out) = std::env::var_os("BAV_SHOP_PICKS") else { panic!("set BAV_SHOP_PICKS") };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let _run = stamp::start("shop_picks", Some(&out));
    stamp::set_aside(&out, "picks.tsv");
    let top: usize = std::env::var("BAV_SHOP_TOP").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let data = GameData::bundled();
    let t0 = std::time::Instant::now();
    let (mut lines, mut times) = (String::new(), String::new());
    let mut n = 0;
    for (name, path) in states() {
        let Some(path) = path else {
            lines += &format!("{name}\t0\t-\t\tnot found on this machine\n");
            continue;
        };
        let run = match save::load(&path, data) {
            Ok(r) => r,
            Err(e) => {
                lines += &format!("{name}\t0\t-\t\tload error: {e}\n");
                continue;
            }
        };
        let t = std::time::Instant::now();
        let a = advise::analyze(&run, data, None, &advise::Options::default());
        eprintln!("{name} {:.1?}", t.elapsed());
        times += &format!("{name}\t{}\n", t.elapsed().as_millis());
        n += 1;
        if a.options.is_empty() {
            lines += &format!("{name}\t0\t-\t\tno shop options\n");
        }
        for (i, o) in a.options.iter().take(top).enumerate() {
            let (judged, why) = match judge_option(o, &run, data) {
                Ok(s) => (s, String::new()),
                Err(e) => ("-".into(), e),
            };
            lines += &format!("{name}\t{}\t{judged}\t{}\t{why}\n", i + 1, o.label.replace('\t', " "));
        }
    }
    std::fs::write(out.join("picks.tsv"), lines).unwrap();
    std::fs::write(out.join("times.tsv"), times).unwrap();
    println!("{n} states ({:.0?})", t0.elapsed());
}
