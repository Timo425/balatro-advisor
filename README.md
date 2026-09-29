# balatro-advisor

A local advisor for **Balatro**. It reads your run from the game's own files and
shows, in a small window that updates as you play:

- your **chance to beat** each blind of the ante, simulated with your deck, hand
  levels and jokers, plus the boss's effect and each blind's reward and skip tag;
- how much of your score **each joker** carries, what selling it would cost, and the
  **best joker order** when order matters;
- **your options**: everything you can buy right now (jokers, planets, tarots, packs,
  rerolls, vouchers) ranked by your win chance afterwards, with the money and interest
  each leaves you. While a pack is open, its picks come first;
- **worth digging for**: every joker and tarot the shop can still offer, ranked for this
  run, with the odds of seeing one that gets you to a target win chance;
- a **style outlook**: which play style (flushes, pairs, held cards, …) has the most room,
  as the best two jokers each could add, against a boss two antes ahead;
- **money valued in the same terms**: what extra cash would buy over the coming shops
  (Hermit, Temperance, economy vouchers);
- the game's own **joker text** on hover (read from your install, with current values),
  and your **Gold Stake sticker** progress.

Scoring is a line-by-line port of the game's own `evaluate_play` and joker code,
covering the scoring effects of all 150 jokers. Win chances come from Monte Carlo
round simulations with a simple play/discard policy (a heuristic, not perfect play).
Long-range numbers (the style outlook, money value) are estimates, and are labelled as such.

## Install and run

Needs Rust ([rustup](https://rustup.rs)).

```bash
git clone https://github.com/Timo425/balatro-advisor && cd balatro-advisor
cargo install --path crates/cli
balatro-advisor ui          # opens the advisor window (Chrome/Chromium app window, else your browser)
```

The save folder is found automatically on Linux (Steam + Proton) and Windows
(`%APPDATA%\Balatro`). Override with `--save-dir`, the `BALATRO_DIR` env var, or
`save_dir` in `~/.config/balatro-advisor/config.toml`.

### Optional: live updates (Lovely mod)

Without a mod, the game only writes its save at checkpoints (entering the shop,
each new hand…), so buys, sells and consumables show up late. The included
**advisor-live** mod writes the current state twice a second instead:

1. Install [Lovely](https://github.com/ethangreen-dev/lovely-injector) (on Linux/Proton,
   Steam launch option `WINEDLLOVERRIDES="version=n,b" %command%`).
2. Copy `mod/advisor-live` into `%APPDATA%\Balatro\Mods\`.

It only serialises the run the same way the game's own `save_run()` does; it doesn't
change gameplay. Don't combine it with Steamodded if you care about achievements:
Steamodded turns them off, Lovely alone does not.

## Other commands

```bash
balatro-advisor analyze            # the full analysis as JSON (for scripts / AI agents)
balatro-advisor state [--json]     # the parsed run
balatro-advisor gold               # Gold Stake stickers you're missing
balatro-advisor score --hand 1,2,5 --trace          # score cards from your hand, step by step
balatro-advisor score "KS KH:glass 5D:stone" --held "KD:steel"
balatro-advisor bench              # engine timings
```

Card notation: `KS`, `10H`, plus `:modifiers`: `bonus mult wild glass steel stone gold
lucky`, `foil holo poly`, `red blue goldseal purple`, `debuff`, `+N` (perma chips).

**Golden tests:** before playing a hand, `balatro-advisor score --hand 1,2,5 --golden name`;
after it scores, `balatro-advisor golden set name <score the game showed>`. `cargo test`
keeps checking it.

`data/game.json` (joker/blind/tag numbers) is generated from a local install:
`unzip -p .../Balatro.exe game.lua > /tmp/game.lua && cargo run -- extract-data /tmp/game.lua`.

## Notes

- Written almost entirely with an AI coding assistant (Claude Code), reviewed and
  play-tested by a human.
- Real save/profile files are never committed; tests use synthetic ones.
- Not affiliated with LocalThunk or Playstack. Balatro is © LocalThunk. This repo
  ships no game code or assets; `data/game.json` holds only names and numbers.
- MIT licensed, see [LICENSE](LICENSE).
