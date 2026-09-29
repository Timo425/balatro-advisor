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
    State,
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
    },
    /// Live advisor page in your browser, updating whenever the game saves
    Ui {
        #[arg(long, default_value_t = 7777)]
        port: u16,
        /// Don't open the browser
        #[arg(long)]
        no_open: bool,
    },
    /// Time the engine on fixed synthetic boards
    Bench,
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
        Cmd::State => {
            let dir = dir()?;
            let s = save::load(&save::run_path(&dir, profile(&dir)), data)?;
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
            let floor = engine::score(&board, &played, &held, &mut Unlucky, *trace);
            let ceil = engine::score(&board, &played, &held, &mut Lucky, false);
            let mut rng = Rng::new(42);
            let n = 2000;
            let mean = if floor.score == ceil.score {
                floor.score
            } else {
                (0..n).map(|_| engine::score(&board, &played, &held, &mut rng, false).score).sum::<f64>() / n as f64
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
                    predicted: floor.score,
                    expected: None,
                    note: String::new(),
                };
                let p = case.save(std::path::Path::new(GOLDEN_DIR)).map_err(anyhow::Error::msg)?;
                println!("\nSaved {}. After playing it: balatro-advisor golden set {name} <score the game showed>", p.display());
            }
        }
        Cmd::Analyze { sims, seed } => {
            let dir = dir()?;
            let p = profile(&dir);
            let run = save::load(&save::run_path(&dir, p), data)?;
            let g = gold::load(&dir, p, data).ok();
            let opts = balatro_advisor::advise::Options { sims: *sims, seed: *seed, ..Default::default() };
            let a = balatro_advisor::advise::analyze(&run, data, g.as_ref(), &opts);
            println!("{}", serde_json::to_string_pretty(&a)?);
        }
        Cmd::Ui { port, no_open } => {
            let dir = dir()?;
            let p = profile(&dir);
            ui::run(dir, p, *port, !*no_open)?;
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
                    if got == *score {
                        println!("✓ {name}: engine and game agree on {score}");
                    } else {
                        println!("✗ {name}: engine {got}, game {score}. `balatro-advisor golden show {name}` for the trace");
                    }
                }
                GoldenCmd::Show { name } => {
                    let c = golden::Case::load(&golden::Case::path(dir, name)).map_err(anyhow::Error::msg)?;
                    let o = engine::score(&c.board, &c.played, &c.held, &mut Unlucky, true);
                    print_score(&c.board, &c.played, &c.held, &o, o.score, o.score);
                    println!("Game showed: {}", c.expected.map_or("not recorded".into(), |e| e.to_string()));
                }
                GoldenCmd::List => {
                    for (p, c) in golden::load_all(dir) {
                        match c {
                            Ok(c) => {
                                let got = c.rescore();
                                let mark = match c.expected {
                                    None => "…",
                                    Some(e) if e == got => "✓",
                                    Some(_) => "✗",
                                };
                                println!("{mark} {:<24} engine {got:>12}  game {}", c.name, c.expected.map_or("-".into(), |e| e.to_string()));
                            }
                            Err(e) => println!("! {}: {e}", p.display()),
                        }
                    }
                }
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
    println!("Score: {} ({} × {})", fmt_num(o.score), fmt_num(o.chips), fmt_num(o.mult));
    if ceil != o.score {
        println!("  random effects: {} if every roll fails, {} if all hit, mean ≈ {}", fmt_num(o.score), fmt_num(ceil), fmt_num(mean.round()));
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
    println!(
        "${} | hands {} discards {} | hand size {} | jokers {}/{}",
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
