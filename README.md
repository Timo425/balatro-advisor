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

> **Status:** save/profile reader and `gold` work; the scoring engine is next.
> Design notes: [docs/plan.md](docs/plan.md), [docs/formats.md](docs/formats.md).

```bash
cargo build --release
./target/release/balatro-advisor gold          # Gold Stake stickers missing
./target/release/balatro-advisor state         # parsed run (add --json for everything)
cargo test                                     # ~1 s
```

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
