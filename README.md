# balatro-advisor

A fast local advisor for **vanilla Balatro**. It reads the game's own save file
(no mods, achievements stay on) and tells you:

1. how much each of your jokers contributes to your score,
2. the best order to put them in,
3. which shop or candidate joker would improve the run most (as an addition,
   or as a swap when your slots are full),

plus your **Gold Stake sticker progress** from the profile file.

Scoring follows the game's real evaluation order. It is evaluated with Monte
Carlo over your remaining deck, with fixed seeds, so the results are
reproducible. Jokers that aren't modelled yet are shown as **not modelled**,
never silently left out.

> **Status:** save reader, `gold` and the scoring engine (`score`) work. The
> engine is a line-by-line port of the game's `evaluate_play` and covers the
> scoring effects of all 150 jokers. Monte Carlo (`analyze`/`shop`) is next.
> Design notes: [docs/plan.md](docs/plan.md), [docs/formats.md](docs/formats.md).

```bash
cargo build --release
B=./target/release/balatro-advisor
$B gold                           # Gold Stake stickers missing
$B state                          # parsed run (add --json for everything)
$B score --hand 1,2,5 --trace     # score cards from your hand, step by step
$B score "KS KH:glass 5D:stone" --held "KD:steel"
cargo test                        # ~2 s
```

Card notation: `KS`, `10H`, plus `:modifiers`: `bonus mult wild glass steel
stone gold lucky`, `foil holo poly`, `red blue goldseal purple`, `debuff`,
`+N` (perma chips).

**Golden tests** (real in-game scores): before playing a hand run
`$B score --hand 1,2,5 --golden my-case`; after it scores run
`$B golden set my-case <score the game showed>`. `cargo test` checks every
recorded case from then on; `$B golden list` shows them.

`data/game.json` (joker/blind numbers) is generated from your own install:
`unzip -p ~/.steam/debian-installation/steamapps/common/Balatro/Balatro.exe game.lua > /tmp/game.lua && cargo run -- extract-data /tmp/game.lua`

## Planned interfaces

```bash
balatro-advisor analyze   # joker contributions + best order + P(beat blind)
balatro-advisor shop      # rank shop / --candidate jokers by marginal gain
balatro-advisor gold      # Gold Stake stickers: have / missing
balatro-advisor watch     # re-run automatically when save.jkr changes
balatro-advisor bench     # performance benchmark
balatro-advisor score     # score one specified hand, with --trace
# every command: --json --profile N --save-dir PATH --seed N --samples N

balatro-advisor-mcp       # MCP server (stdio) for AI agents, same JSON
```

## Save location

Found automatically on Linux (Steam + Proton), and in `%APPDATA%\Balatro` on
Windows. Override with `--save-dir`, the `BALATRO_DIR` env var, or `save_dir`
in `~/.config/balatro-advisor/config.toml`.

## Privacy

Real `.jkr` files (saves, profiles, settings) are never committed. They are
read in place and gitignored. Test fixtures are synthetic.

## Not affiliated

Not affiliated with LocalThunk or Playstack. Balatro is © LocalThunk. The tool
only reads your local save files. It does not modify the game or ship any of
its code or assets.
