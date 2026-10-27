//! The parity tier (docs/decisions.md D17): the engine's steps checked lockstep against the game's
//! own Lua, run headless from the owner's install (`tools/parity/`: a stand-in for LÖVE and a job
//! runner, LuaJIT). On a fixed sample of the owner's saved states (`~/.local/share/
//! balatro-advisor/history`, chosen by name as the history check's is, `history_picks::sample`)
//! it makes jobs: the history's own steps (a hand, a round-winning hand with its cards inferred,
//! a discard, a blind set, a cash-out run on from the winning hand, a shop reroll, a pack opened),
//! and generated ones from each state in a hand (the play the engine scores most, a random play,
//! a random discard, seeded by the state's name). The runner loads each state through the game's
//! `Game:start_run`, takes the step through its button functions and writes the game's own save of
//! what it leaves; the game rolls its own RNG from the save, so a roll is the game's.
//!
//! Checked: the harness against the game's next save (the history's steps: the harness itself),
//! and the engine against the harness: every hand's score (`handlog`'s verdict: rolls as the range
//! between every roll failing and every roll hitting), the state a hand leaves
//! (`Board::after_hand`), a discard's (`Board::discard`), a blind's setup (`Board::at_blind_set`).
//! Every difference is put in a known group (rolls in range, Riff-raff's random jokers at a
//! blind's setup the engine's seeds didn't draw, the bosses gap 15 plays as plain blinds, steps
//! not modelled yet, the owner acting between saves, the saves' timing, card ids) or listed as
//! NEW; the test fails on a new one, a score mismatch or a job the harness couldn't run. Run
//! where a change touches `engine/` or a `Board` step the simulated round plays, before and
//! after (CLAUDE.md step 4), and after a game update:
//! `BAV_PARITY_DIR=/tmp/before nice -n 19 cargo test --release --test parity -- --ignored parity
//! --nocapture`, then `diff -r` the two `summary.txt`. `BAV_PARITY_SAMPLE` states (default 8000,
//! about 15 minutes on 6 runners; 0: all, today's too), `BAV_WORKERS` runners (default half the
//! cores), `BAV_ONLY` as the history check's. Skips with a message where LuaJIT, `unzip`, the install, the profile or the history
//! is missing. Never copies the game's code: the runner reads it from `Balatro.exe` in place.

#[path = "history_picks.rs"]
mod history_picks;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use balatro_advisor::data::GameData;
use balatro_advisor::engine::{self, Board, Given, Lucky, Unlucky};
use balatro_advisor::handlog::{self, Game, Pairing, Verdict};
use balatro_advisor::model::{Card, Enhancement};
use balatro_advisor::save::{self, CardIds, RunState, Screen};
use balatro_advisor::sim::RoundStart;
use balatro_advisor::{describe, jkr, paths};
use history_picks::name;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Action {
    Play,
    Discard,
    SelectBlind,
    PlayCashOut,
    Reroll,
    OpenPack,
}

impl Action {
    fn runner(self) -> &'static str {
        match self {
            Action::Play => "play",
            Action::Discard => "discard",
            Action::SelectBlind => "select_blind",
            Action::PlayCashOut => "play_cash_out",
            Action::Reroll => "reroll",
            Action::OpenPack => "open_pack",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum How {
    /// The history's own step
    Replay,
    /// The history's round-winning hand, its cards inferred (the save after it has none)
    Won,
    /// The play the engine scores most
    Best,
    Random,
}

impl How {
    fn label(self) -> &'static str {
        match self {
            How::Replay => "the history's",
            How::Won => "the history's, won the round (cards inferred)",
            How::Best => "generated: the engine's best play",
            How::Random => "generated: random",
        }
    }
}

struct Job {
    action: Action,
    how: How,
    before: PathBuf,
    /// The game's own save after the step (the history's steps)
    after: Option<PathBuf>,
    /// The cards in play order, or the booster's place
    ids: Vec<i64>,
}

impl Job {
    fn kind(&self) -> String {
        format!("{} ({})", self.action.runner(), self.how.label())
    }
}

// ---------------------------------------------------------------------------------------------
// The jobs

/// xorshift, seeded by a state's name
struct Xs(u64);

impl Xs {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) | 1
}

/// Sets of 1 to 5 of `n` cards, as places in the hand
fn subsets(n: usize) -> Vec<Vec<usize>> {
    (1u32..(1 << n)).filter(|m| m.count_ones() <= 5).map(|m| (0..n).filter(|i| m & (1 << i) != 0).collect()).collect()
}

fn split(hand: &[Card], places: &[usize]) -> (Vec<Card>, Vec<Card>) {
    let played = places.iter().map(|&i| hand[i]).collect();
    let held = (0..hand.len()).filter(|i| !places.contains(i)).map(|i| hand[i]).collect();
    (played, held)
}

/// The generated steps from a state in a hand
fn generated(p: &Path, a: &RunState, aids: &CardIds, data: &GameData, jobs: &mut Vec<Job>) {
    if a.screen != Screen::SelectingHand || a.hand.is_empty() || a.hands_left <= 0 {
        return;
    }
    let Some(ids) = aids.hand_ids().filter(|v| v.len() == a.hand.len()) else { return };
    let mut rng = Xs(fnv(&name(p)));
    let board = Board::from_run(a, data);
    let subs = subsets(a.hand.len().min(12));
    let score = |s: &Vec<usize>| {
        let (played, held) = split(&a.hand, s);
        engine::score_as_played(&board, &played, &held, &mut Unlucky, false).score
    };
    let best = subs.iter().max_by(|x, y| score(x).total_cmp(&score(y))).expect("a hand has a card");
    let mut push = |action, how, s: &[usize]| jobs.push(Job { action, how, before: p.to_path_buf(), after: None, ids: s.iter().map(|&i| ids[i]).collect() });
    push(Action::Play, How::Best, best);
    push(Action::Play, How::Random, &subs[rng.below(subs.len())]);
    if a.discards_left > 0 {
        push(Action::Discard, How::Random, &subs[rng.below(subs.len())]);
    }
}

/// The history's own step from `b` to `a`, the save after it
fn replayed(b: &RunState, bids: &CardIds, a: &RunState, aids: &CardIds, data: &GameData) -> Option<(Action, How, Vec<i64>)> {
    if a.seed != b.seed {
        return None;
    }
    match handlog::pair(b, bids, a, aids, data, 0) {
        Pairing::Hand(h) => return Some((Action::Play, How::Replay, h.ids)),
        Pairing::Skipped(_) if b.screen == Screen::SelectingHand && a.screen == Screen::RoundEval && a.hands_played == b.hands_played + 1 => {
            return won_hand(b, bids, a, data).map(|ids| (Action::Play, How::Won, ids));
        }
        _ => {}
    }
    let same_round = b.screen == Screen::SelectingHand && a.screen == Screen::SelectingHand && a.round == b.round && a.hands_played == b.hands_played;
    if same_round && a.discards_left + 1 == b.discards_left {
        let (hb, db, ha, da) = (bids.hand_ids()?, bids.discard_ids()?, aids.hand_ids()?, aids.discard_ids()?);
        let leaving: Vec<i64> = hb.iter().copied().filter(|i| !ha.contains(i)).collect();
        // in the order they reached the pile; a card gone from both was destroyed (Trading Card)
        let mut order: Vec<i64> = da.iter().copied().filter(|i| !db.contains(i)).collect();
        let rest: Vec<i64> = leaving.iter().copied().filter(|i| !order.contains(i)).collect();
        order.extend(rest);
        return (!leaving.is_empty() && leaving.len() <= 5).then_some((Action::Discard, How::Replay, order));
    }
    if b.screen == Screen::BlindSelect && a.screen == Screen::SelectingHand && a.hands_played == b.hands_played && a.discards_used == 0 {
        return Some((Action::SelectBlind, How::Replay, vec![]));
    }
    let (Screen::Shop, Some(bs), Some(sa)) = (b.screen, b.shop.as_ref(), a.shop.as_ref()) else { return None };
    let keys = |v: &[save::JokerCard]| v.iter().map(|j| j.key.clone()).collect::<Vec<_>>();
    // a reroll: the money its cost, the joker row changed, nothing bought
    if a.screen == Screen::Shop && (b.dollars - a.dollars - bs.reroll_cost as f64).abs() < 1e-9 && a.jokers.len() == b.jokers.len() && a.consumables.len() == b.consumables.len() && keys(&bs.jokers) != keys(&sa.jokers) {
        return Some((Action::Reroll, How::Replay, vec![]));
    }
    // a pack opened: its booster gone from the shop
    if !a.open_pack.is_empty() && sa.boosters.len() + 1 == bs.boosters.len() {
        let after: Vec<&str> = sa.boosters.iter().map(|x| x.key.as_str()).collect();
        let i = (0..bs.boosters.len()).find(|&i| {
            let mut k: Vec<&str> = bs.boosters.iter().map(|x| x.key.as_str()).collect();
            k.remove(i);
            k == after
        })?;
        return Some((Action::OpenPack, How::Replay, vec![i as i64 + 1]));
    }
    None
}

/// The cards of a round-winning hand: the one set of cards (as cards, not ids) of the hand type
/// whose count went up that reaches what the blind needed; none where more than one fits
fn won_hand(b: &RunState, bids: &CardIds, a: &RunState, data: &GameData) -> Option<Vec<i64>> {
    let typ = a.hand_levels.iter().find(|(k, l)| b.hand_levels.get(*k).is_some_and(|x| l.played == x.played + 1)).and_then(|(k, _)| engine::HandType::from_name(k))?;
    let (hb, cb) = (bids.hand_ids()?, b.current_blind.as_ref()?);
    let board = Board::from_run(b, data);
    if engine::hook_takes(&board, b.hand.len().saturating_sub(1)) > 0 {
        return None;
    }
    let need = cb.target - cb.scored;
    let mut fits: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
    for sub in subsets(b.hand.len().min(12)) {
        let (played, held) = split(&b.hand, &sub);
        let o = engine::score_as_played(&board, &played, &held, &mut Lucky, false);
        if o.hand == typ && o.score >= need * (1.0 - 1e-9) {
            let mut k: Vec<String> = played.iter().map(|c| format!("{c:?}")).collect();
            k.sort();
            fits.entry(k).or_insert(sub);
        }
    }
    (fits.len() == 1).then(|| fits.into_values().next().expect("one").iter().map(|&i| hb[i]).collect())
}

fn jobs(sample: &[PathBuf], data: &GameData) -> Vec<Job> {
    let mut by_run: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for p in sample {
        by_run.entry(p.parent().expect("in a run's folder").to_path_buf()).or_default().push(p.clone());
    }
    let mut out = Vec::new();
    for (run, picked) in by_run {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&run).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jkr")).collect();
        files.sort();
        for p in picked {
            let Ok((a, aids)) = save::load_with_ids(&p, data) else { continue };
            generated(&p, &a, &aids, data, &mut out);
            let i = files.iter().position(|f| *f == p).expect("listed");
            let Some((n1, (s1, ids1))) = files.get(i + 1).and_then(|f| Some((f, save::load_with_ids(f, data).ok()?))) else { continue };
            let Some((action, how, ids)) = replayed(&a, &aids, &s1, &ids1, data) else { continue };
            out.push(Job { action, how, before: p.clone(), after: Some(n1.clone()), ids: ids.clone() });
            // a winning hand: the cash-out run on from it (a cash-out screen loaded from its own
            // save has lost the blind's reward), against the shop's first save after the
            // cash-out screen's
            if action == Action::Play && s1.screen == Screen::RoundEval {
                for n2 in &files[i + 2..] {
                    let Ok((s2, _)) = save::load_with_ids(n2, data) else { break };
                    if s2.seed != s1.seed || s2.round != s1.round || !matches!(s2.screen, Screen::RoundEval | Screen::Shop) {
                        break;
                    }
                    if s2.screen == Screen::Shop {
                        out.push(Job { action: Action::PlayCashOut, how, before: p.clone(), after: Some(n2.clone()), ids: ids.clone() });
                        break;
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The runners

struct Setup {
    tools: PathBuf,
    exe: PathBuf,
    /// Plain-text copies of the owner's settings and profile
    profile: PathBuf,
    work: PathBuf,
}

fn runs(cmd: &str) -> bool {
    Command::new(cmd).arg("-v").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Plain text of a `.jkr` (or the file as it is, if it isn't deflated)
fn inflated(p: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(p)?;
    Ok(jkr::decode(&bytes).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned()))
}

fn setup() -> Result<Setup, String> {
    if !runs("luajit") {
        return Err("LuaJIT isn't installed (`sudo apt install luajit`, D17)".into());
    }
    if !runs("unzip") {
        return Err("`unzip` isn't installed".into());
    }
    let exe = describe::game_exe().ok_or("Balatro.exe not found (`BALATRO_EXE`)")?;
    let save_dir = paths::resolve_save_dir(None).map_err(|e| format!("the game's save folder: {e}"))?;
    let n = paths::active_profile(&save_dir);
    if !save_dir.join(n.to_string()).join("profile.jkr").is_file() {
        return Err(format!("no profile {n} in {}", save_dir.display()));
    }
    let work = std::env::temp_dir().join(format!("bav-parity-{}", std::process::id()));
    let profile = work.join("profile");
    std::fs::create_dir_all(profile.join(n.to_string())).map_err(|e| e.to_string())?;
    // read only: plain-text copies the stand-in reads, the owner's files untouched
    for f in ["settings.jkr".to_string(), format!("{n}/profile.jkr"), format!("{n}/meta.jkr")] {
        if let Ok(text) = inflated(&save_dir.join(&f)) {
            std::fs::write(profile.join(&f), text).map_err(|e| e.to_string())?;
        }
    }
    let tools = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity");
    Ok(Setup { tools, exe, profile, work })
}

/// What the runner made of a job
enum Ran {
    /// The chips the step scored, and the game's save of what it left
    Ok { chips: f64, after: Box<(RunState, CardIds)> },
    /// A move the game doesn't take (Cerulean Bell's card left out, two cards sharing an id)
    Skip(String),
    Fail(String),
}

struct Runner<'a> {
    setup: &'a Setup,
    k: usize,
    proc: Option<(Child, ChildStdin, BufReader<ChildStdout>)>,
}

impl Runner<'_> {
    fn input(&self) -> PathBuf {
        self.setup.work.join(format!("in-{}.lua", self.k))
    }

    fn output(&self) -> PathBuf {
        self.setup.work.join(format!("out-{}.lua", self.k))
    }

    fn start(&mut self) -> std::io::Result<()> {
        let mut c = Command::new("luajit")
            .arg("runner.lua")
            .current_dir(&self.setup.tools)
            .env("PARITY_EXE", &self.setup.exe)
            .env("PARITY_PROFILE", &self.setup.profile)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let (i, o) = (c.stdin.take().expect("piped"), BufReader::new(c.stdout.take().expect("piped")));
        self.proc = Some((c, i, o));
        Ok(())
    }

    fn run(&mut self, job: &Job, data: &GameData) -> Ran {
        let text = match inflated(&job.before) {
            Ok(t) => t,
            Err(e) => return Ran::Fail(format!("reading the state: {e}")),
        };
        let (input, output) = (self.input(), self.output());
        let _ = std::fs::remove_file(&output);
        if let Err(e) = std::fs::write(&input, text) {
            return Ran::Fail(format!("writing the state: {e}"));
        }
        if self.proc.is_none()
            && let Err(e) = self.start()
        {
            return Ran::Fail(format!("starting luajit: {e}"));
        }
        let ids = job.ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let (_, stdin, stdout) = self.proc.as_mut().expect("started");
        let mut line = String::new();
        let sent = writeln!(stdin, "{}\t{}\t{ids}\t{}", input.display(), job.action.runner(), output.display()).and_then(|_| stdin.flush());
        if sent.is_err() || stdout.read_line(&mut line).map_or(true, |n| n == 0) {
            // the runner died: started again for the next job
            if let Some((mut c, ..)) = self.proc.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
            return Ran::Fail("the runner died".into());
        }
        let f: Vec<&str> = line.trim_end().split('\t').collect();
        match f[0] {
            "ok" => {
                let chips = f.get(1).and_then(|x| x.parse().ok()).unwrap_or(f64::NAN);
                match std::fs::read_to_string(&output).map_err(|e| e.to_string()).and_then(|t| save::from_text_with_ids(&t, data, &output).map_err(|e| e.to_string())) {
                    Ok(after) => Ran::Ok { chips, after: Box::new(after) },
                    Err(e) => Ran::Fail(format!("the harness's save: {e}")),
                }
            }
            "skip" => Ran::Skip(f.get(1).unwrap_or(&"").to_string()),
            _ => Ran::Fail(f.get(1).unwrap_or(&"").to_string()),
        }
    }
}

impl Drop for Runner<'_> {
    fn drop(&mut self) {
        if let Some((mut c, stdin, _)) = self.proc.take() {
            drop(stdin);
            let _ = c.wait();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The comparison

/// One difference: the field, what each side has, and the known group it falls in (none: new)
struct Diff {
    field: String,
    what: String,
    known: Option<&'static str>,
}

const ROLL: &str = "a roll, in range (between the engine's every roll failing and every roll hitting)";
const GLASS: &str = "a Glass card broke: no engine step breaks one (card.lua `Card:shatter`), not modelled yet";
const GAP15: &str = "a boss gap 15 plays as a plain blind (Amber Acorn, Crimson Heart)";
const RANDOM_PICK: &str = "Riff-raff's jokers, drawn at random at a blind's setup: none of the engine's 256 seeds drew the game's";
const SETTING_BLIND: &str = "Ceremonial Dagger's and Madness's `setting_blind` (a joker destroyed, their growth), not modelled yet (gap 8, gap 26)";
const OWNER: &str = "the owner acted between the two saves (a sale, a consumable used, jokers moved)";
const TIMING: &str = "the game saved before the step finished (a tag still pending, the shop not stocked yet)";
const IDS: &str = "card ids (copies sharing one; a new card's id comes from a counter the save doesn't keep)";
const FLOAT: &str = "one chip off: the game floors a float its save rounds (Ramen's ×Mult)";

const GAP15_BOSSES: [&str; 2] = ["bl_final_acorn", "bl_final_heart"];
const SETTING_BLIND_JOKERS: [&str; 2] = ["j_ceremonial", "j_madness"];

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

/// Numeric (and other non-list) leaves of two JSON values that differ, by path
fn json_diff(path: &str, a: &Value, b: &Value, out: &mut Vec<(String, String)>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            if !close(x, y) {
                out.push((path.to_string(), format!("{x} / {y}")));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                if let Some(w) = y.get(k) {
                    json_diff(&format!("{path}.{k}"), v, w, out);
                }
            }
        }
        (x, y) if x != y && !x.is_array() => out.push((path.to_string(), format!("{x} / {y}"))),
        _ => {}
    }
}

/// What differs between two boards, as (field, "first / second")
fn board_diff(e: &Board, g: &Board, money: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (ek, gk): (Vec<&str>, Vec<&str>) = (e.jokers.iter().map(|j| j.key.as_str()).collect(), g.jokers.iter().map(|j| j.key.as_str()).collect());
    if ek != gk {
        out.push(("jokers".into(), format!("{ek:?} / {gk:?}")));
    } else {
        for (x, y) in e.jokers.iter().zip(&g.jokers) {
            let (mut xv, yv) = (serde_json::to_value(x).expect("serialises"), serde_json::to_value(y).expect("serialises"));
            // a joker's sell value isn't a step's
            xv["sell_value"] = yv["sell_value"].clone();
            json_diff(&x.key, &xv, &yv, &mut out);
        }
    }
    for (i, (x, y)) in e.levels.iter().zip(&g.levels).enumerate() {
        let h = engine::HandType::ALL[i].name();
        for (f, a, b) in [("level", x.level as f64, y.level as f64), ("chips", x.chips, y.chips), ("mult", x.mult, y.mult), ("played", x.played as f64, y.played as f64), ("played_this_round", x.played_this_round as f64, y.played_this_round as f64)] {
            if !close(a, b) {
                out.push((format!("level.{f}"), format!("{h} {a} / {b}")));
            }
        }
    }
    let mut num = |f: &str, a: f64, b: f64| {
        if !close(a, b) {
            out.push((f.to_string(), format!("{a} / {b}")));
        }
    };
    if money {
        num("dollars", e.dollars, g.dollars);
    }
    num("hands_left", e.hands_left as f64, g.hands_left as f64);
    num("discards_left", e.discards_left as f64, g.discards_left as f64);
    num("discards_used", e.discards_used as f64, g.discards_used as f64);
    num("hands_played", e.hands_played as f64, g.hands_played as f64);
    num("playing_cards", e.playing_cards as f64, g.playing_cards as f64);
    num("eye_seen", e.blind.eye_seen as f64, g.blind.eye_seen as f64);
    if e.blind.mouth_only != g.blind.mouth_only {
        out.push(("mouth_only".into(), format!("{:?} / {:?}", e.blind.mouth_only, g.blind.mouth_only)));
    }
    out
}

fn cards_sorted<'a>(cards: impl Iterator<Item = &'a Card>) -> Vec<String> {
    let mut v: Vec<String> = cards.map(|c| format!("{c:?}")).collect();
    v.sort();
    v
}

/// What the harness's save after a step and the game's own differ in besides the board: the
/// cards (by id), the screen, and what the game drew (the shop, a pack, the blinds and tags
/// ahead, consumables, the round's targets)
fn save_diff(h: &(RunState, CardIds), g: &(RunState, CardIds)) -> Vec<Diff> {
    let mut d = Vec::new();
    let mut push = |field: &str, what: String, known| d.push(Diff { field: field.into(), what, known });
    let sorted = |v: &[Option<i64>]| {
        let mut v = v.to_vec();
        v.sort();
        v
    };
    if sorted(&h.1.hand) != sorted(&g.1.hand) {
        let same = cards_sorted(h.0.hand.iter()) == cards_sorted(g.0.hand.iter());
        push("cards.hand", format!("{:?} / {:?}", h.1.hand, g.1.hand), same.then_some(IDS));
    }
    if sorted(&h.1.discard) != sorted(&g.1.discard) {
        let same = cards_sorted(h.0.discard_pile.iter()) == cards_sorted(g.0.discard_pile.iter());
        push("cards.discard", format!("{} / {} cards", h.1.discard.len(), g.1.discard.len()), same.then_some(IDS));
    }
    if h.0.draw_pile.len() != g.0.draw_pile.len() {
        push("cards.deck", format!("{} / {}", h.0.draw_pile.len(), g.0.draw_pile.len()), None);
    }
    if h.0.screen != g.0.screen {
        push("screen", format!("{:?} / {:?}", h.0.screen, g.0.screen), None);
    }
    let drawn = |s: &RunState| -> Vec<(&'static str, String)> {
        let keys = |v: &mut dyn Iterator<Item = String>| v.collect::<Vec<_>>().join(",");
        let mut v = Vec::new();
        if let Some(sh) = &s.shop {
            v.push(("shop.jokers", keys(&mut sh.jokers.iter().map(|j| format!("{}{:?}", j.key, j.edition)))));
            v.push(("shop.other", keys(&mut sh.other_cards.iter().map(|j| j.key.clone()))));
            v.push(("shop.boosters", keys(&mut sh.boosters.iter().map(|j| j.key.clone()))));
            v.push(("shop.vouchers", keys(&mut sh.vouchers.iter().map(|j| j.key.clone()))));
            v.push(("shop.reroll_cost", sh.reroll_cost.to_string()));
        }
        v.push(("open_pack", keys(&mut s.open_pack.iter().map(|j| j.key.clone()))));
        v.push(("blinds", keys(&mut s.blinds.iter().map(|b| format!("{}:{}:{:?}", b.key, b.state, b.skip_tag)))));
        v.push(("tags", s.tags.join(",")));
        v.push(("consumables", keys(&mut s.consumables.iter().map(|c| c.key.clone()))));
        let t = &s.round_targets;
        v.push(("round_targets", format!("{:?}", (t.idol, t.mail_rank, t.castle_suit, t.ancient_suit))));
        v
    };
    let (dh, dg) = (drawn(&h.0), drawn(&g.0));
    for (x, y) in dh.iter().zip(&dg) {
        if x.0 == y.0 && x.1 != y.1 {
            push(x.0, format!("[{}] / [{}]", x.1, y.1), None);
        }
    }
    // the cards' own state (enhancement, seal, edition, bonus, debuff), by id
    let by_id = |s: &(RunState, CardIds)| -> BTreeMap<i64, Card> { s.1.hand.iter().zip(&s.0.hand).filter_map(|(i, c)| i.map(|i| (i, *c))).collect() };
    let (ch, cg) = (by_id(h), by_id(g));
    if let Some((c, gc)) = ch.iter().find_map(|(i, c)| cg.get(i).filter(|gc| *gc != c).map(|gc| (c, gc))) {
        push("cards.state", format!("{} {c:?} / {gc:?}", gc.label()), None);
    }
    d
}

/// The owner acted between the two saves: a consumable gone or a joker gone from the game's
/// save that the step left, or the same jokers in another order
fn owner_acted(h: &RunState, g: &RunState) -> bool {
    let gone = |a: Vec<String>, b: Vec<String>| {
        let mut rest = b;
        a.into_iter().any(|k| match rest.iter().position(|x| *x == k) {
            Some(i) => {
                rest.remove(i);
                false
            }
            None => true,
        })
    };
    let (hj, gj): (Vec<String>, Vec<String>) = (h.jokers.iter().map(|j| j.key.clone()).collect(), g.jokers.iter().map(|j| j.key.clone()).collect());
    let moved = hj != gj && hj.len() == gj.len() && !gone(hj.clone(), gj.clone());
    moved || gone(hj, gj) || gone(h.consumables.iter().map(|c| c.key.clone()).collect(), g.consumables.iter().map(|c| c.key.clone()).collect())
}

/// The game saved before the step finished: a tag still on the game's save that the harness's
/// used, or the game's shop not stocked yet
fn early_save(h: &RunState, g: &RunState) -> bool {
    let stocked = |s: &RunState| s.shop.as_ref().is_some_and(|sh| !sh.jokers.is_empty() || !sh.boosters.is_empty() || !sh.vouchers.is_empty());
    let pending = g.tags.len() > h.tags.len() && h.tags.iter().all(|t| g.tags.contains(t));
    pending || (stocked(h) && g.shop.is_some() && !stocked(g))
}

/// The engine's state check of one step
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    AfterHand,
    Discard,
    AtBlindSet,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Step::AfterHand => "Board::after_hand",
            Step::Discard => "Board::discard",
            Step::AtBlindSet => "Board::at_blind_set",
        }
    }
}

/// What one job showed
#[derive(Default)]
struct Judged {
    kind: String,
    name: String,
    /// The runner skipped the move or failed
    ran: Option<Result<(), (bool, String)>>,
    /// The harness against the game's next save
    vs_game: Option<Vec<Diff>>,
    /// The engine's score: the verdict, and the hand's summary where it's a mismatch
    score: Option<(String, Option<String>)>,
    /// The engine against the harness: the step checked, its differences or why it isn't checked
    state: Option<(Step, Result<Vec<Diff>, &'static str>)>,
}

fn judge(job: &Job, ran: Ran, data: &GameData) -> Judged {
    let mut out = Judged { kind: job.kind(), name: name(&job.before), ..Default::default() };
    let (chips, h) = match ran {
        Ran::Ok { chips, after } => (chips, *after),
        Ran::Skip(why) => {
            out.ran = Some(Err((true, why)));
            return out;
        }
        Ran::Fail(why) => {
            out.ran = Some(Err((false, why)));
            return out;
        }
    };
    out.ran = Some(Ok(()));
    let Ok(b) = save::load_with_ids(&job.before, data) else {
        out.ran = Some(Err((false, "the state before won't load".into())));
        return out;
    };
    // 1. the harness against the game, where the history has the game's own save after the step
    if let Some(Ok(g)) = job.after.as_ref().map(|p| save::load_with_ids(p, data)) {
        let mut d: Vec<Diff> = board_diff(&Board::from_run(&h.0, data), &Board::from_run(&g.0, data), true).into_iter().map(|(field, what)| Diff { field, what, known: None }).collect();
        d.extend(save_diff(&h, &g));
        if job.action == Action::Play
            && let (Some(hc), Some(gc)) = (h.0.current_blind.as_ref(), g.0.current_blind.as_ref())
            && !close(hc.scored, gc.scored)
        {
            let off = (hc.scored - gc.scored).abs() <= 1.0;
            d.push(Diff { field: "score".into(), what: format!("{} / {}", hc.scored, gc.scored), known: off.then_some(FLOAT) });
        }
        let group = if owner_acted(&h.0, &g.0) {
            Some(OWNER)
        } else if early_save(&h.0, &g.0) {
            Some(TIMING)
        } else {
            None
        };
        for x in &mut d {
            x.known = x.known.or(group);
        }
        out.vs_game = Some(d);
    }
    // 2. the engine against the harness
    let board = Board::from_run(&b.0, data);
    let cards = |ids: &[i64]| -> Option<Vec<Card>> {
        let hb = b.1.hand_ids()?;
        ids.iter().map(|id| hb.iter().position(|x| x == id).map(|i| b.0.hand[i])).collect()
    };
    match job.action {
        Action::Play | Action::PlayCashOut => {
            if job.action == Action::PlayCashOut {
                return out;
            }
            let dup = |ids: &CardIds| {
                let mut seen = BTreeSet::new();
                ids.hand.iter().chain(&ids.discard).flatten().any(|i| !seen.insert(*i))
            };
            if dup(&b.1) || dup(&h.1) {
                out.state = Some((Step::AfterHand, Err("two cards share an id (a copy): which was played is ambiguous")));
                return out;
            }
            let (Some(played), Some(hb)) = (cards(&job.ids), b.1.hand_ids()) else {
                out.state = Some((Step::AfterHand, Err("cards not in the hand")));
                return out;
            };
            let held: Vec<Card> = hb.iter().enumerate().filter(|(_, id)| !job.ids.contains(id)).map(|(i, _)| b.0.hand[i]).collect();
            // The Hook's pick: from the pile, as the history's hands are paired
            let hooked = if engine::hook_takes(&board, held.len()) > 0 {
                match handlog::pair(&b.0, &b.1, &h.0, &h.1, data, 0) {
                    Pairing::Hand(x) => x.hooked,
                    _ => {
                        out.state = Some((Step::AfterHand, Err("The Hook's pick isn't in the harness's save (the round was won)")));
                        return out;
                    }
                }
            } else {
                vec![]
            };
            let hand = handlog::Hand::new(&b.0, board.clone(), played, held, hooked, Game::Scored(chips), vec![], 0);
            let v = hand.verdict();
            let verdict = match v {
                Verdict::Match => "match",
                Verdict::InRange => "in range (a roll)",
                Verdict::Enough => "enough",
                Verdict::Mismatch => "MISMATCH",
            };
            out.score = Some((verdict.to_string(), (v == Verdict::Mismatch).then(|| format!("{}: {}", hand.cause(), hand.summary()))));
            if h.0.screen != Screen::SelectingHand {
                out.state = Some((Step::AfterHand, Err("the hand won the round (the round's end isn't a step)")));
                return out;
            }
            let step = |lucky: bool| {
                let mut e = board.clone();
                let o = if lucky { engine::score_as_played(&e, &hand.played, &hand.held, &mut Lucky, false) } else { engine::score_as_played(&e, &hand.played, &hand.held, &mut Given(hand.hooked.clone()), false) };
                e.after_hand(&o);
                board_diff(&e, &Board::from_run(&h.0, data), true)
            };
            let (lo, hi) = (step(false), step(true));
            // a value between the engine's with every roll failing and with every roll hitting is
            // a roll the game made
            let nums = |w: &str| -> Option<(f64, f64)> {
                let (x, y) = w.split_once(" / ")?;
                Some((x.rsplit(' ').next()?.parse().ok()?, y.parse().ok()?))
            };
            let glass = hand.played.iter().any(|c| c.enhancement == Some(Enhancement::Glass));
            let boss = gap15(&board.blind.key, board.blind.disabled);
            let d = lo
                .into_iter()
                .map(|(field, what)| {
                    let rolled = nums(&what).is_some_and(|(x, y)| {
                        let xh = hi.iter().find(|(q, _)| *q == field).and_then(|(_, w)| nums(w)).map_or(y, |(xh, _)| xh);
                        y >= x.min(xh) - 1e-9 && y <= x.max(xh) + 1e-9
                    });
                    let known = if field == "playing_cards" && glass {
                        Some(GLASS)
                    } else if rolled {
                        Some(ROLL)
                    } else {
                        boss
                    };
                    Diff { field, what, known }
                })
                .collect();
            out.state = Some((Step::AfterHand, Ok(d)));
        }
        Action::Discard => {
            let Some(c) = cards(&job.ids) else {
                out.state = Some((Step::Discard, Err("cards not in the hand")));
                return out;
            };
            let mut e = board.clone();
            e.discard(&c);
            let boss = gap15(&board.blind.key, board.blind.disabled);
            let d = board_diff(&e, &Board::from_run(&h.0, data), true).into_iter().map(|(field, what)| Diff { field, what, known: boss }).collect();
            out.state = Some((Step::Discard, Ok(d)));
        }
        Action::SelectBlind => {
            let Some(cb) = h.0.current_blind.as_ref() else {
                out.state = Some((Step::AtBlindSet, Err("no blind in the harness's save")));
                return out;
            };
            let start = RoundStart {
                hand: h.0.hand.iter().map(|c| Card { debuff: false, face_down: false, ..*c }).collect(),
                deck: vec![],
                hand_size: b.0.hand_size,
                hands: b.0.round_hands,
                discards: b.0.round_discards,
                scored: 0.0,
                target: cb.target,
                fresh: true,
                blind: cb.key.clone(),
                disabled: cb.disabled,
                juggle: b.0.round_start_tags(),
            };
            let g = Board::from_run(&h.0, data);
            let setup = |seed: u64| {
                let (mut e, s) = board.at_blind_set_drawn(&start, seed);
                let mut g = g.clone();
                // the blind's rules and the deck's size aren't this step's (the save's blind is
                // read as the engine reads it; the cards dealt are the harness's)
                g.blind = e.blind.clone();
                e.hands_left = s.hands;
                e.discards_left = s.discards;
                e.playing_cards = g.playing_cards;
                let mut d = board_diff(&e, &g, false);
                if s.hand_size != h.0.round_hand_size() {
                    d.push(("hand_size".into(), format!("{} / {}", s.hand_size, h.0.round_hand_size())));
                }
                if let Some((ec, gc)) = s.hand.iter().zip(&h.0.hand).find(|(ec, gc)| ec.debuff != gc.debuff) {
                    d.push(("card debuff".into(), format!("{} {} / {}", gc.label(), ec.debuff, gc.debuff)));
                }
                (d, e.jokers.len())
            };
            // Riff-raff's jokers, a random pick: the engine's seeds tried for the game's, the
            // closest kept; what's left is the pick's where it's the jokers made (their keys,
            // their numbers, what they give while held) and the jokers count the same
            let random = board.jokers.iter().any(|j| j.key == "j_riff_raff");
            let mut best = setup(0);
            for seed in 1..if random { 256 } else { 1 } {
                if best.0.is_empty() {
                    break;
                }
                let next = setup(seed);
                if next.0.len() < best.0.len() {
                    best = next;
                }
            }
            let made: Vec<&engine::Joker> = if random && best.1 == g.jokers.len() { g.jokers.iter().skip(board.jokers.len()).collect() } else { vec![] };
            let held_mods = made.iter().any(|j| j.round_mods() != (0, 0, 0));
            let of_made = |field: &str| field == "jokers" || made.iter().any(|j| field.starts_with(j.key.as_str())) || (held_mods && matches!(field, "hand_size" | "discards_left"));
            let boss = gap15(&cb.key, cb.disabled);
            let setting = board.jokers.iter().any(|j| SETTING_BLIND_JOKERS.contains(&j.key.as_str()) && !j.debuff);
            let d = best
                .0
                .into_iter()
                .map(|(field, what)| {
                    let known = boss.or(setting.then_some(SETTING_BLIND)).or((!made.is_empty() && of_made(&field)).then_some(RANDOM_PICK));
                    Diff { field, what, known }
                })
                .collect();
            out.state = Some((Step::AtBlindSet, Ok(d)));
        }
        Action::Reroll | Action::OpenPack => {}
    }
    out
}

fn gap15(key: &str, disabled: bool) -> Option<&'static str> {
    (!disabled && GAP15_BOSSES.contains(&key)).then_some(GAP15)
}

// ---------------------------------------------------------------------------------------------
// The summary

/// Steps of one kind checked: how many, how many the same, the known groups and the new
/// differences (by field), each with its examples
#[derive(Default)]
struct Tally {
    checked: usize,
    same: usize,
    skipped: BTreeMap<String, usize>,
    known: BTreeMap<&'static str, BTreeSet<String>>,
    new: BTreeMap<String, BTreeSet<String>>,
}

impl Tally {
    fn add(&mut self, d: &[Diff], name: &str) {
        self.checked += 1;
        if d.is_empty() {
            self.same += 1;
        }
        for x in d {
            match x.known {
                Some(k) => self.known.entry(k).or_default().insert(name.to_string()),
                None => self.new.entry(x.field.clone()).or_default().insert(format!("{name}: {}", x.what)),
            };
        }
    }

    fn write(&self, title: &str, out: &mut String) {
        out.push_str(&format!("{title}: {} checked, {} the same\n", self.checked, self.same));
        for (why, n) in &self.skipped {
            out.push_str(&format!("    not checked {n}: {why}\n"));
        }
        for (k, names) in &self.known {
            out.push_str(&format!("    known {}: {k}\n", names.len()));
            for n in names {
                out.push_str(&format!("        {n}\n"));
            }
        }
        for (f, ex) in &self.new {
            out.push_str(&format!("    NEW {} {f}\n", ex.len()));
            for e in ex {
                out.push_str(&format!("        {e}\n"));
            }
        }
    }
}

#[derive(Default)]
struct Report {
    vs_game: BTreeMap<String, Tally>,
    scores: BTreeMap<String, BTreeMap<String, usize>>,
    mismatches: BTreeSet<String>,
    state: BTreeMap<(Step, String), Tally>,
    skipped: BTreeMap<String, usize>,
    failed: BTreeMap<String, BTreeSet<String>>,
}

impl Report {
    fn add(&mut self, j: Judged) {
        match j.ran {
            Some(Err((true, why))) => *self.skipped.entry(format!("{}: {why}", j.kind)).or_default() += 1,
            Some(Err((false, why))) => {
                self.failed.entry(format!("{}: {}", j.kind, why.split(" | ").next().unwrap_or(""))).or_default().insert(j.name.clone());
            }
            _ => {}
        }
        if let Some(d) = &j.vs_game {
            self.vs_game.entry(j.kind.clone()).or_default().add(d, &j.name);
        }
        if let Some((v, m)) = j.score {
            *self.scores.entry(j.kind.clone()).or_default().entry(v).or_default() += 1;
            if let Some(m) = m {
                self.mismatches.insert(format!("{}: {m}", j.name));
            }
        }
        if let Some((step, r)) = j.state {
            let t = self.state.entry((step, j.kind.clone())).or_default();
            match r {
                Ok(d) => t.add(&d, &j.name),
                Err(why) => *t.skipped.entry(why.to_string()).or_default() += 1,
            }
        }
    }

    fn new_count(&self) -> usize {
        self.vs_game.values().chain(self.state.values()).map(|t| t.new.values().map(BTreeSet::len).sum::<usize>()).sum::<usize>() + self.mismatches.len() + self.failed.values().map(BTreeSet::len).sum::<usize>()
    }

    fn text(&self, head: &str) -> String {
        let mut out = format!("{head}\n");
        let total = |m: &BTreeMap<String, Tally>| m.values().fold((0, 0), |(c, s), t| (c + t.checked, s + t.same));
        let (c, s) = total(&self.vs_game);
        out.push_str(&format!("\n== The harness against the game's next save (the history's steps): {s} of {c} the same\n"));
        for (k, t) in &self.vs_game {
            t.write(k, &mut out);
        }
        let hands: usize = self.scores.values().flat_map(|m| m.values()).sum();
        let rolled: usize = self.scores.values().filter_map(|m| m.get("in range (a roll)")).sum();
        out.push_str(&format!("\n== The engine's scores against the harness: {hands} hands, {} mismatches, {rolled} with a roll in range\n", self.mismatches.len()));
        for (k, v) in &self.scores {
            out.push_str(&format!("{k}: {}\n", v.iter().map(|(x, n)| format!("{x} {n}")).collect::<Vec<_>>().join(", ")));
        }
        for m in &self.mismatches {
            out.push_str(&format!("    MISMATCH {m}\n"));
        }
        out.push_str("\n== The engine's state after each step against the harness\n");
        let mut by_step: BTreeMap<Step, (usize, usize)> = BTreeMap::new();
        for ((step, _), t) in &self.state {
            let e = by_step.entry(*step).or_default();
            e.0 += t.checked;
            e.1 += t.same;
        }
        for (step, (c, s)) in &by_step {
            out.push_str(&format!("{}: {s} of {c} the same\n", step.name()));
        }
        for ((step, k), t) in &self.state {
            t.write(&format!("{} after {k}", step.name()), &mut out);
        }
        out.push_str("\n== Moves the game doesn't take (generated)\n");
        for (k, n) in &self.skipped {
            out.push_str(&format!("{n:5}  {k}\n"));
        }
        out.push_str("\n== Jobs the harness couldn't run\n");
        for (k, names) in &self.failed {
            out.push_str(&format!("    FAILED {} {k}\n", names.len()));
            for n in names {
                out.push_str(&format!("        {n}\n"));
            }
        }
        out
    }
}

/// The tier (D17). Slow: 11 minutes on the 6,270 states saved by 2026-10-23 (9,293 jobs, 6
/// runners), about 15 at the default sample. Run under `nice -n 19` (the runners inherit it).
#[test]
#[ignore]
fn parity() {
    let setup = match setup() {
        Ok(s) => s,
        Err(why) => {
            eprintln!("parity: skipped: {why}");
            return;
        }
    };
    let data = GameData::bundled();
    let size: usize = std::env::var("BAV_PARITY_SAMPLE").ok().and_then(|s| s.parse().ok()).unwrap_or(8000);
    let sample = history_picks::sample(size);
    if sample.is_empty() {
        eprintln!("parity: skipped: no saved history (`~/.local/share/balatro-advisor/history`)");
        return;
    }
    let t0 = std::time::Instant::now();
    let jobs = jobs(&sample, data);
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for j in &jobs {
        *kinds.entry(j.kind()).or_default() += 1;
    }
    let head = format!(
        "parity: {} states sampled, {} jobs\n{}",
        sample.len(),
        jobs.len(),
        kinds.iter().map(|(k, n)| format!("    {n:5}  {k}")).collect::<Vec<_>>().join("\n")
    );
    eprintln!("parity: {} states sampled, {} jobs made in {:.0?}", sample.len(), jobs.len(), t0.elapsed());
    let workers: usize = std::env::var("BAV_WORKERS").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).max(1)));
    let next = AtomicUsize::new(0);
    let report = Mutex::new(Report::default());
    std::thread::scope(|s| {
        for k in 0..workers {
            let (setup, jobs, next, report) = (&setup, &jobs, &next, &report);
            s.spawn(move || {
                let mut r = Runner { setup, k, proc: None };
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let judged = judge(job, r.run(job, data), data);
                    report.lock().expect("not poisoned").add(judged);
                    if (i + 1) % 1000 == 0 {
                        eprintln!("parity: {} of {} jobs ({:.0?})", i + 1, jobs.len(), t0.elapsed());
                    }
                }
            });
        }
    });
    let _ = std::fs::remove_dir_all(&setup.work);
    let report = report.into_inner().expect("not poisoned");
    let text = report.text(&head);
    println!("{text}");
    eprintln!("parity: {} jobs on {workers} runners in {:.0?}", jobs.len(), t0.elapsed());
    if let Some(dir) = std::env::var_os("BAV_PARITY_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("summary.txt"), &text).unwrap();
    }
    let n = report.new_count();
    assert!(n == 0, "{n} differences outside the known groups, score mismatches or failed jobs: every one explained or fixed (CLAUDE.md step 4)");
}
