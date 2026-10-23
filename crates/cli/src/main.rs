use std::path::PathBuf;

use anyhow::{Context, Result};
use balatro_advisor::data::GameData;
use balatro_advisor::engine::{self, Board, Lucky, Rng, Unlucky};
use balatro_advisor::model::Card;
use balatro_advisor::{gold, golden, paths, save};
use clap::{Parser, Subcommand};

mod ui;

#[derive(Parser)]
#[command(name = "balatro-advisor", version, about = "Joker values, ordering and shop advice from your Balatro save")]
struct Cli {
    /// Balatro save directory (default: BALATRO_DIR, config file, or auto-discovery)
    #[arg(long, global = true)]
    save_dir: Option<PathBuf>,
    /// Profile 1-3 (default: the one selected in the game)
    #[arg(long, global = true)]
    profile: Option<u8>,
    /// Machine-readable output
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Gold Stake sticker progress
    Gold,
    /// The parsed run state
    State {
        /// Read this save file instead of the profile's (e.g. one from the page's history, to
        /// capture as a replay fixture)
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Score one hand against your current board (jokers, levels, money… from the save)
    Score {
        /// Cards to play, e.g. "KS KH:glass 5D:stone" (see README for modifiers)
        cards: Option<String>,
        /// Pick the played cards from your current hand by position (1 = leftmost), e.g. 1,3,4
        #[arg(long)]
        hand: Option<String>,
        /// Cards held in hand (default with --hand: the rest of your hand)
        #[arg(long)]
        held: Option<String>,
        /// Ignore the save: no jokers, level 1 hands
        #[arg(long)]
        no_save: bool,
        /// Print every step of the scoring pass
        #[arg(long)]
        trace: bool,
        /// Save as a golden case to check against the game's real score later
        #[arg(long)]
        golden: Option<String>,
    },
    /// Full analysis of the current run: joker values, order, shop, rescue jokers, blind odds
    Analyze {
        /// Round simulations per board
        #[arg(long, default_value_t = 300)]
        sims: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Analyse this save file instead of the profile's (e.g. one the bot wrote)
        #[arg(long)]
        file: Option<PathBuf>,
        /// In a blind, only what the play decision needs (much faster; for bots)
        #[arg(long)]
        quick: bool,
    },
    /// Simulate a plan against the blinds ahead, next to the board as it is. For "what if I…"
    /// questions (meant for agents too): e.g. --sell mystic_summit,misprint
    /// --jokers crafty,red_card,red_card --card "4D:lucky:red=4D:glass:red"
    Whatif {
        /// The whole joker list after the plan; owned jokers keep their values, naming one
        /// twice copies it (Ankh). Keys or names, with :foil/:holo/:poly/:negative
        #[arg(long, value_delimiter = ',')]
        jokers: Option<Vec<String>>,
        /// Jokers to sell
        #[arg(long, value_delimiter = ',')]
        sell: Vec<String>,
        /// Jokers to add (fresh from the shop)
        #[arg(long, value_delimiter = ',')]
        add: Vec<String>,
        /// A card changed, "AS KH" notation: "OLD=NEW", e.g. "4D:lucky:red=4D:glass:red" (repeatable)
        #[arg(long = "card")]
        cards: Vec<String>,
        /// Cards added to the deck, e.g. "2C:poly:wild"
        #[arg(long)]
        add_cards: Option<String>,
        /// Cards removed (Hanged Man), e.g. "2C 3S"
        #[arg(long)]
        remove_cards: Option<String>,
        #[arg(long, default_value_t = 2000)]
        sims: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Live advisor page in your browser, updating whenever the game saves
    Ui {
        #[arg(long, default_value_t = 7777)]
        port: u16,
        /// Don't open the browser
        #[arg(long)]
        no_open: bool,
    },
    /// How well the advisor's win chances matched what happened (logged while `ui` runs)
    Calibration,
    /// Time the engine on fixed synthetic boards
    Bench,
    /// Hands you played, the engine's score beside the game's (logged while `ui` runs): what
    /// didn't match, each numbered for --capture
    Hands {
        /// Pair the saved history's states instead of reading the log
        #[arg(long)]
        history: bool,
        /// List every hand, not only the mismatches
        #[arg(long)]
        all: bool,
        /// Save hand number N (as listed) as a golden case in tests/golden/
        #[arg(long, requires = "name")]
        capture: Option<usize>,
        /// The golden case's name
        #[arg(long)]
        name: Option<String>,
        /// Mark the captured case as a known engine bug (e.g. "queue card 3"): cargo test skips it
        /// while it disagrees
        #[arg(long)]
        pending: Option<String>,
    },
    /// Golden cases: real hands with the in-game score
    Golden {
        #[command(subcommand)]
        cmd: GoldenCmd,
    },
    /// Regenerate data/game.json from the game's game.lua
    /// (`unzip -p .../Balatro/Balatro.exe game.lua > /tmp/game.lua`)
    ExtractData {
        game_lua: PathBuf,
        #[arg(long, default_value = "data/game.json")]
        out: PathBuf,
        #[arg(long, default_value = "1.0.1o-FULL")]
        game_version: String,
    },
}

#[derive(Subcommand)]
enum GoldenCmd {
    /// Record the score the game showed for a captured case
    Set { name: String, score: f64 },
    /// Show a case with the engine's trace
    Show { name: String },
    /// List cases and whether they match
    List,
}

const GOLDEN_DIR: &str = "tests/golden";

fn main() -> Result<()> {
    // `balatro-advisor gold | head` should end quietly, not panic on a closed pipe.
    #[cfg(unix)]
    unsafe {
        libc_sigpipe_default();
    }
    let cli = Cli::parse();
    let data = GameData::bundled();
    let dir = || -> Result<PathBuf> { Ok(paths::resolve_save_dir(cli.save_dir.as_deref())?) };
    let profile = |dir: &std::path::Path| {
        cli.profile.or(paths::Config::load().profile).unwrap_or_else(|| paths::active_profile(dir))
    };

    match &cli.cmd {
        Cmd::Gold => {
            let dir = dir()?;
            let r = gold::load(&dir, profile(&dir), data)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print_gold(&r);
            }
        }
        Cmd::State { file } => {
            let s = match file {
                Some(f) => save::load(f, data)?,
                None => {
                    let dir = dir()?;
                    save::load(&save::run_path(&dir, profile(&dir)), data)?
                }
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print_state(&s);
            }
        }
        Cmd::Score { cards, hand, held, no_save, trace, golden: golden_name } => {
            let run = if *no_save {
                None
            } else {
                let dir = dir()?;
                Some(save::load(&save::run_path(&dir, profile(&dir)), data)?)
            };
            let mut board = run.as_ref().map_or_else(Board::empty, |r| Board::from_run(r, data));
            let (played, held) = match (hand, cards) {
                (Some(idx), _) => {
                    let r = run.as_ref().context("--hand needs a save")?;
                    if r.hand.is_empty() {
                        anyhow::bail!("the save has no hand right now ({:?}); --hand works while you're in a blind", r.screen);
                    }
                    let picks: Vec<usize> = idx
                        .split(',')
                        .map(|n| n.trim().parse::<usize>().context("--hand takes positions like 1,3,4"))
                        .collect::<Result<_>>()?;
                    let mut played = Vec::new();
                    let mut rest = Vec::new();
                    for (i, c) in r.hand.iter().enumerate() {
                        if picks.contains(&(i + 1)) { played.push(*c) } else { rest.push(*c) }
                    }
                    let held = match held {
                        Some(h) => Card::parse_list(h).map_err(anyhow::Error::msg)?,
                        None => rest,
                    };
                    (played, held)
                }
                (None, Some(c)) => (
                    Card::parse_list(c).map_err(anyhow::Error::msg)?,
                    held.as_deref().map(Card::parse_list).transpose().map_err(anyhow::Error::msg)?.unwrap_or_default(),
                ),
                (None, None) => anyhow::bail!("give the cards to play, or --hand 1,2,3 to pick from your hand"),
            };
            if played.is_empty() || played.len() > 5 {
                anyhow::bail!("play 1 to 5 cards (got {})", played.len());
            }
            if run.is_some() && hand.is_none() {
                // Cards typed by hand: the draw pile the save shows is still the best guess for Blue Joker.
                board.deck_remaining = board.deck_remaining.max(0);
            }
            // A golden case is checked against the game's score for the cards as you played and
            // held them; otherwise they're scored in the order that scores most (and it's shown)
            let score_fn = if golden_name.is_some() { engine::score_as_played::<dyn engine::Rolls> } else { engine::score::<dyn engine::Rolls> };
            let floor = score_fn(&board, &played, &held, &mut Unlucky, *trace);
            let ceil = score_fn(&board, &played, &held, &mut Lucky, false);
            let mut rng = Rng::new(42);
            let n = 2000;
            let mean = if floor.score == ceil.score {
                floor.score
            } else {
                (0..n).map(|_| score_fn(&board, &played, &held, &mut rng, false).score).sum::<f64>() / n as f64
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "outcome": floor, "if_all_rolls_hit": ceil.score, "mean": mean,
                }))?);
            } else {
                print_score(&board, &played, &held, &floor, ceil.score, mean);
            }
            if let Some(name) = golden_name {
                let case = golden::Case {
                    name: name.clone(),
                    board,
                    played,
                    held,
                    hooked: vec![],
                    predicted: floor.score,
                    expected: None,
                    pending: None,
                    note: String::new(),
                };
                let p = case.save(std::path::Path::new(GOLDEN_DIR)).map_err(anyhow::Error::msg)?;
                println!("\nSaved {}. After playing it: balatro-advisor golden set {name} <score the game showed>", p.display());
            }
        }
        Cmd::Analyze { sims, seed, file, quick } => {
            let (run, g) = match file {
                Some(f) => (save::load(f, data)?, None),
                None => {
                    let dir = dir()?;
                    let p = profile(&dir);
                    (save::load(&save::run_path(&dir, p), data)?, gold::load(&dir, p, data).ok())
                }
            };
            let opts = balatro_advisor::advise::Options { sims: *sims, seed: *seed, quick: *quick, ..Default::default() };
            let a = balatro_advisor::advise::analyze(&run, data, g.as_ref(), &opts);
            println!("{}", serde_json::to_string_pretty(&a)?);
        }
        Cmd::Whatif { jokers, sell, add, cards, add_cards, remove_cards, sims, seed } => {
            let dir = dir()?;
            let run = save::load(&save::run_path(&dir, profile(&dir)), data)?;
            let parse = |s: &Option<String>| s.as_deref().map(Card::parse_list).transpose().map_err(anyhow::Error::msg).map(Option::unwrap_or_default);
            let set_cards = cards
                .iter()
                .map(|c| {
                    let (a, b) = c.split_once('=').context("--card takes OLD=NEW")?;
                    Ok((Card::parse(a.trim()).map_err(anyhow::Error::msg)?, Card::parse(b.trim()).map_err(anyhow::Error::msg)?))
                })
                .collect::<Result<Vec<_>>>()?;
            let plan = balatro_advisor::whatif::Plan {
                jokers: jokers.clone(),
                sell: sell.clone(),
                add: add.clone(),
                set_cards,
                add_cards: parse(add_cards)?,
                remove_cards: parse(remove_cards)?,
            };
            let o = balatro_advisor::whatif::run(&run, data, &plan, *sims, *seed).map_err(anyhow::Error::msg)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&o)?);
            } else {
                println!("Plan board: {}", o.jokers.join(", "));
                for n in &o.notes {
                    println!("⚠ {n}");
                }
                println!("{:<28} {:>10} {:>22} {:>22}", "", "target", "now", "plan");
                for (a, b) in o.now.iter().zip(&o.plan) {
                    let cell = |x: &balatro_advisor::whatif::BlindOdds| {
                        let bones = x.p_with_bones.map_or(String::new(), |p| format!(" ({:.0}%)", p * 100.0));
                        format!("{:.1}%{bones} · {:.0}% · ${:.1}", x.p_win * 100.0, x.reach * 100.0, x.round_money)
                    };
                    println!("{:<28} {:>10.0} {:>22} {:>22}", a.label, a.target, cell(a), cell(b));
                }
                let m = &o.money;
                println!("{:<28} {:>10} {:>22} {:>22}", "Money now", "", format!("${:.0}", m.dollars), format!("${:.0}", m.dollars_after));
                let signed = |x: f64| format!("{}${:.0}", if x < 0.0 { "−" } else { "+" }, x.abs());
                println!("{:<28} {:>10} {:>22} {:>22}", "Jokers pay a round (− rent)", "", signed(m.per_round), signed(m.per_round_after));
                println!("(win chance on score, Mr. Bones' save in brackets · average share of the target reached · money the round makes while played: Lucky, Gold cards; {sims} rounds each. Score odds don't see money: compare the money rows for jokers that pay)");
            }
        }
        Cmd::Ui { port, no_open } => {
            let dir = dir()?;
            let p = profile(&dir);
            ui::run(dir, p, *port, !*no_open)?;
        }
        Cmd::Calibration => {
            use balatro_advisor::calibration::{self, Entry};
            let dir = calibration::default_dir().context("no data directory")?;
            let entries = calibration::read(&dir);
            let buckets = calibration::report(&entries);
            let wins: Vec<&Entry> = entries.iter().filter(|e| matches!(e, Entry::Win { .. })).collect();
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "buckets": buckets, "wins": wins }))?);
            } else {
                let total: usize = buckets.iter().map(|b| b.n).sum();
                println!("Blinds with a recorded outcome: {total} (log: {})", dir.join("calibration.jsonl").display());
                for b in buckets.iter().filter(|b| b.n > 0) {
                    println!(
                        "  predicted {:>3.0}–{:<3.0}%: won {:>3} of {:<3} ({:.0}%, avg prediction {:.0}%)",
                        b.from * 100.0,
                        b.to * 100.0,
                        (b.actual * b.n as f64).round(),
                        b.n,
                        b.actual * 100.0,
                        b.predicted * 100.0
                    );
                }
                if total < 30 {
                    println!("  (needs a few dozen blinds before it means much)");
                }
                println!("Won runs recorded: {}", wins.len());
            }
        }
        Cmd::Bench => {
            for t in balatro_advisor::bench::run() {
                println!("{:<44} {:>9.1} ms  ({:.1} µs each)", t.name, t.ms, t.per_item_us);
            }
        }
        Cmd::Golden { cmd } => {
            let dir = std::path::Path::new(GOLDEN_DIR);
            match cmd {
                GoldenCmd::Set { name, score } => {
                    let mut c = golden::Case::load(&golden::Case::path(dir, name)).map_err(anyhow::Error::msg)?;
                    c.expected = Some(*score);
                    c.save(dir).map_err(anyhow::Error::msg)?;
                    let got = c.rescore();
                    if c.agrees() == Some(true) {
                        println!("✓ {name}: engine and game agree on {score}");
                    } else {
                        println!("✗ {name}: engine {got}, game {score}. `balatro-advisor golden show {name}` for the trace");
                    }
                }
                GoldenCmd::Show { name } => {
                    let c = golden::Case::load(&golden::Case::path(dir, name)).map_err(anyhow::Error::msg)?;
                    let o = c.outcome(true);
                    print_score(&c.board, &c.played, &c.held, &o, o.score, o.score);
                    println!("Game showed: {}", c.expected.map_or("not recorded".into(), |e| e.to_string()));
                }
                GoldenCmd::List => {
                    for (p, c) in golden::load_all(dir) {
                        match c {
                            Ok(c) => {
                                let got = c.rescore();
                                let mark = match (c.agrees(), &c.pending) {
                                    (None, _) => "…",
                                    (Some(true), _) => "✓",
                                    (Some(false), Some(_)) => "◌",
                                    (Some(false), None) => "✗",
                                };
                                println!("{mark} {:<24} engine {got:>12}  game {}", c.name, c.expected.map_or("-".into(), |e| e.to_string()));
                            }
                            Err(e) => println!("! {}: {e}", p.display()),
                        }
                    }
                }
            }
        }
        Cmd::Hands { history, all, capture, name, pending } => {
            use balatro_advisor::handlog::{self, Verdict};
            let dir = balatro_advisor::calibration::default_dir().context("no data directory")?;
            let (hands, scan) = if *history {
                let mut s = handlog::scan(&dir.join("history"), data);
                (std::mem::take(&mut s.hands), Some(s))
            } else {
                (handlog::read_log(&dir), None)
            };
            if let Some(n) = capture {
                let h = hands.get(n.wrapping_sub(1)).with_context(|| format!("no hand {n} (there are {})", hands.len()))?;
                let mut c = h.golden(name.as_deref().unwrap_or_default()).context("that hand matched, and the log keeps no board for it: capture it from --history")?;
                c.pending = pending.clone();
                let p = c.save(std::path::Path::new(GOLDEN_DIR)).map_err(anyhow::Error::msg)?;
                let state = match c.agrees() {
                    None => "no exact game score (the hand won the blind): fill it in with `golden set`".to_string(),
                    Some(true) => "engine and game agree".to_string(),
                    Some(false) => format!("engine {}, game {}{}", c.rescore(), c.expected.unwrap_or_default(), if c.pending.is_some() { " (pending)" } else { " (cargo test fails until fixed, or mark it --pending)" }),
                };
                println!("Saved {}: {state}", p.display());
                return Ok(());
            }
            if cli.json {
                let rows: Vec<_> = hands.iter().enumerate().map(|(i, h)| serde_json::json!({ "n": i + 1, "verdict": h.verdict(), "cause": h.cause(), "hand": h })).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "hands": rows, "scan": scan }))?);
                return Ok(());
            }
            let count = |v: Verdict| hands.iter().filter(|h| h.verdict() == v).count();
            let source = if *history { format!("the history ({})", dir.join("history").display()) } else { handlog::log_path(&dir).display().to_string() };
            println!("Hands compared: {} from {source}", hands.len());
            println!(
                "  match exactly {} · a roll involved, in range {} · won the blind, engine reaches it {} · mismatch {}",
                count(Verdict::Match),
                count(Verdict::InRange),
                count(Verdict::Enough),
                count(Verdict::Mismatch)
            );
            if let Some(s) = &scan {
                println!("  from {} saved states; the runs played {} hands in all. Not compared:", s.states, s.hands_played);
                for (k, n) in &s.skipped {
                    println!("    {n:>5}  {}", k.describe());
                }
                let seen = hands.len() + s.skipped.iter().map(|x| x.1).sum::<usize>();
                println!("    {:>5}  no save of the state before or after it (the run ended on it, or the page missed it)", (s.hands_played as usize).saturating_sub(seen));
            }
            let mut causes: Vec<(String, Vec<usize>)> = Vec::new();
            for (i, h) in hands.iter().enumerate().filter(|(_, h)| *all || h.verdict() == Verdict::Mismatch) {
                let c = if h.verdict() == Verdict::Mismatch { h.cause() } else { format!("{:?}", h.verdict()).to_lowercase() };
                match causes.iter_mut().find(|x| x.0 == c) {
                    Some(x) => x.1.push(i),
                    None => causes.push((c, vec![i])),
                }
            }
            for (cause, idx) in &causes {
                println!("\n{cause} ({}):", idx.len());
                for &i in idx {
                    let h = &hands[i];
                    println!("  #{:<4} {}", i + 1, h.summary());
                    for n in &h.notes {
                        println!("        {n}");
                    }
                }
            }
            if !causes.is_empty() {
                println!("\nbalatro-advisor hands{} --capture N --name NAME saves one as a golden case", if *history { " --history" } else { "" });
            }
        }
        Cmd::ExtractData { game_lua, out, game_version } => {
            let src = std::fs::read_to_string(game_lua).with_context(|| format!("reading {}", game_lua.display()))?;
            let d = balatro_advisor::data::extract_from_game_lua(&src, game_version).map_err(anyhow::Error::msg)?;
            std::fs::write(out, serde_json::to_string_pretty(&d)? + "\n")?;
            println!("{} centers ({} jokers), {} blinds → {}", d.centers.len(), d.jokers().count(), d.blinds.len(), out.display());
        }
    }
    Ok(())
}

fn print_gold(r: &gold::GoldReport) {
    println!("Profile {}: Gold sticker on {}/{} jokers, {} missing", r.profile, r.have_gold, r.total, r.missing.len());
    if !r.tally_ok() {
        println!(
            "⚠ Sticker tally {} disagrees with the game's own {:?}: the profile format may have changed.",
            r.tally_computed, r.tally_in_file
        );
    }
    const STAKES: [&str; 9] = ["none", "White", "Red", "Green", "Black", "Blue", "Purple", "Orange", "Gold"];
    for rarity in ["Common", "Uncommon", "Rare", "Legendary"] {
        let list: Vec<_> = r.missing.iter().filter(|j| j.rarity == rarity).collect();
        if list.is_empty() {
            continue;
        }
        println!("\n{rarity} ({})", list.len());
        for j in list {
            println!("  {:<22} best: {:<7} used {}×", j.name, STAKES[j.best_stake.min(8) as usize], j.times_used);
        }
    }
}

fn print_score(board: &Board, played: &[Card], held: &[Card], o: &engine::Outcome, ceil: f64, mean: f64) {
    let label = |cs: &[Card]| cs.iter().map(|c| c.label()).collect::<Vec<_>>().join(" ");
    println!("Play: {}", label(played));
    if !held.is_empty() {
        println!("Held: {}", label(held));
    }
    let names: Vec<&str> = board.jokers.iter().map(|j| balatro_advisor::data::GameData::bundled().name(&j.key)).collect();
    println!("Jokers: {}", if names.is_empty() { "none".into() } else { names.join(", ") });
    if o.debuffed_hand {
        println!("{}: blocked by the boss, scores 0", o.hand.name());
        return;
    }
    let scoring: Vec<String> = o.scoring.iter().map(|&i| played[i].label()).collect();
    println!("{} — scoring: {}", o.hand.name(), scoring.join(" "));
    for s in &o.trace {
        println!("  {:<34} {:>10} × {}", s.source, fmt_num(s.chips), fmt_num(s.mult));
    }
    if o.sure != o.score {
        // The Hook's pick averaged (gap 15); the chips, Mult and trace are the costliest pair's
        println!("Score: {} on average over the 2 held cards The Hook may take; surely {} ({} × {}, the trace above)", fmt_num(o.score.round()), fmt_num(o.sure), fmt_num(o.chips), fmt_num(o.mult));
    } else {
        println!("Score: {} ({} × {})", fmt_num(o.score), fmt_num(o.chips), fmt_num(o.mult));
    }
    if let Some(v) = &o.played_order {
        println!("  play them in this order: {}", label(&v.iter().map(|&i| played[i]).collect::<Vec<_>>()));
    }
    if let Some(v) = &o.held_order {
        println!("  hold them in this order, left to right: {}", label(&v.iter().map(|&i| held[i]).collect::<Vec<_>>()));
    }
    if ceil != o.score {
        println!("  random effects: {} if every roll fails (The Hook's pick averaged), {} if all hit, mean ≈ {}", fmt_num(o.score.round()), fmt_num(ceil), fmt_num(mean.round()));
    }
    if o.dollars > 0.0 {
        println!("  earns ${}", o.dollars);
    }
}

fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        let s = format!("{}", x as i64);
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 && ch != '-' {
                out.push(',');
            }
            out.push(ch);
        }
        out
    } else if x.abs() >= 1e15 {
        format!("{x:.3e}")
    } else {
        format!("{x:.2}")
    }
}

fn joker_label(j: &save::JokerCard) -> String {
    let mut tags: Vec<String> = Vec::new();
    if let Some(e) = j.edition {
        tags.push(format!("{e:?}").to_lowercase());
    }
    if j.eternal {
        tags.push("eternal".into());
    }
    if let Some(n) = j.perishable {
        tags.push(format!("perishable {n} rounds"));
    }
    if j.rental {
        tags.push("rental".into());
    }
    if j.debuff {
        tags.push("DEBUFFED".into());
    }
    if tags.is_empty() { j.name.clone() } else { format!("{} ({})", j.name, tags.join(", ")) }
}

fn print_state(s: &save::RunState) {
    let age = s.snapshot.age_secs.map_or(String::new(), |a| format!(" (saved {}m {}s ago)", a / 60, a % 60));
    println!("{} | stake {} | ante {}/{} round {} | {:?}{age}", s.deck, s.stake, s.ante, s.win_ante, s.round, s.screen);
    for c in &s.snapshot.caveats {
        println!("⚠ {c}");
    }
    let this_round = match s.round_hand_size_delta {
        0 => String::new(),
        d => format!(" ({:+} until the round ends)", d),
    };
    println!(
        "${} | hands {} discards {} | hand size {}{this_round} | jokers {}/{}",
        s.dollars,
        s.hands_left,
        s.discards_left,
        s.hand_size,
        s.jokers.len(),
        s.joker_slots
    );
    for b in &s.blinds {
        println!("  {:<5} {:<14} {:>10} chips  {}", b.slot, b.name, b.target, b.state);
    }
    if let Some(b) = &s.current_blind {
        println!("Current blind: {} {}/{}", b.name, b.scored, b.target);
    }
    println!("Jokers:");
    for j in &s.jokers {
        println!("  {}", joker_label(j));
    }
    if !s.hand.is_empty() {
        println!("Hand: {}", s.hand.iter().map(|c| c.label()).collect::<Vec<_>>().join("  "));
    }
    println!("Draw pile: {} cards", s.draw_pile.len());
    if let Some(shop) = &s.shop {
        println!("Shop:");
        for j in &shop.jokers {
            let tag = j.pending_tag_edition.map_or(String::new(), |e| format!(" → {e:?} from tag"));
            println!("  {} ${}{tag}", joker_label(j), j.cost);
        }
        for c in shop.other_cards.iter().chain(&shop.boosters).chain(&shop.vouchers) {
            println!("  {} ${}", c.name, c.cost);
        }
    }
}

#[cfg(unix)]
unsafe fn libc_sigpipe_default() {
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    unsafe { signal(SIGPIPE, SIG_DFL) };
}
