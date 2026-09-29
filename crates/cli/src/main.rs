use std::path::PathBuf;

use anyhow::{Context, Result};
use balatro_advisor::data::GameData;
use balatro_advisor::{gold, paths, save};
use clap::{Parser, Subcommand};

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
            let s = save::load(&save::save_path(&dir, profile(&dir)), data)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                print_state(&s);
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
