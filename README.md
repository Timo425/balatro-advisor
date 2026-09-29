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

> **Status: phase 1 (research and plan).** No code yet. See
> [docs/plan.md](docs/plan.md) for the design,
> [docs/decisions.md](docs/decisions.md) for the language and dependency
> choices, and [docs/formats.md](docs/formats.md) for the save/profile layout.

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
