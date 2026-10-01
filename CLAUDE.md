# balatro-advisor: working rules

- **Phased work.** The phases are in `docs/plan.md` §9. Stop for owner review
  after each phase.
- **Ask first** before pushing, adding heavy dependencies, or changing scope.
- **Never commit real game files** (`*.jkr`, snapshots of them, extracted game
  source). Read them in place. Private fixtures go in `tests/fixtures/private/`
  (gitignored).
- **Ground truth is the game source.** `Balatro.exe` is a zip; read it with
  `unzip -p ~/.steam/debian-installation/steamapps/common/Balatro/Balatro.exe
  functions/state_events.lua`. Cite the file and function in comments. Never
  copy Lua code into the repo.
- **Joker facts live in `data/`**, not in logic. The `config` numbers come from
  `data/jokers.base.json` (generated). Do not trust the game's `effect` strings.
- **Unmodelled means reported as `not modelled`**, never silently ignored.
- **Heuristics are labelled** in every output (text and JSON).
- **No coupling** to `balatro-agent` or other local repos. Copy and adapt with
  an attribution comment if needed, and never import by path.
- Code, comments and docs in English (D4 in `docs/decisions.md`).

## Answering "what if I…" questions

Use `balatro-advisor whatif` instead of writing scratch programs: it simulates a named
plan against the blinds ahead, next to the board as it is (`--json` for machines).
`--jokers a,b,b` sets the whole joker list (owned ones keep their values; naming one
twice copies it, e.g. Ankh), `--sell` / `--add` change it, `--card "OLD=NEW"` changes a
card in hand or deck ("4D:lucky:red=4D:glass:red"), `--add-cards` / `--remove-cards`.
It plans nothing itself: spotting the line is the human's (or agent's) part.

## Replay fixtures (wrong advice the owner caught)

When the owner says a suggestion is wrong, save that game state before fixing it:
`tests/fixtures/private/<name>.json` = `{"note", "state": <balatro-advisor state --json>,
"expect": {...}}` (keys: `best_action`, `best_hand`, `best_use_first`, `top_option`, `above`: ["A", "B"]).
`cargo test --release --test replay` replays every one; it must pass before a commit.
Other local data (never in the repo): `~/.local/share/balatro-advisor/calibration.jsonl`
(predicted vs actual blind results, shops seen).
