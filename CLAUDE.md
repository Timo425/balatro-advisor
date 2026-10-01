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

## How to improve the advice

Read `docs/design.md` first: the four stages every piece of advice goes through (moves
tried, simulated player, valuation, noise), where each lives, the rules for changing them,
and the register of hand-set values. When advice is wrong, find which stage failed and
improve that stage, so every similar situation gets better at once. If a fix only makes
sense for one joker, suit or hand, it's the wrong fix: look for the step that should have
found it. Add the state as a replay fixture first (below).

**Example.** The advice played a junk Spade instead of digging with the off-suit cards,
because the simulated rest of the round spent the 3♠ Blue Seal in a Flush.
- First fix (wrong scope): "keep Blue Seal cards in hand while on pace". Right step (the
  round policy), but named after the card on screen.
- Right fix: ask what the card is an example of: "cards that pay at the end of the round
  while held". The game defines that group (card.lua `get_end_of_round_effect`: Blue Seal,
  Gold card), so the policy keeps whatever is in it, and Gold cards were covered without
  anyone noticing them.
- Ask the same each time: what general group is this an example of, and where does the game
  (or the simulation) already define it?

**Before every fix, three steps:**
1. Name the stage that failed: *moves tried*, *simulated player*, *valuation* or *noise*
   (`docs/design.md`).
2. Run `git log --oneline | grep -i '<stage>'` (or read the recent log) for earlier fixes
   to that stage. If there is one, the stage itself is the problem: change how it works
   for every case (as "the simulated player plays toward `RoundGoals`" did), don't add
   another rule to it.
3. Start the commit message with the stage, e.g. `simulated player: …`, so step 2 finds it
   next time.

## Replay fixtures (wrong advice the owner caught)

When the owner says a suggestion is wrong, save that game state before fixing it:
`tests/fixtures/private/<name>.json` = `{"note", "state": <balatro-advisor state --json>,
"expect": {...}}` (keys: `best_action`, `best_hand`, `best_use_first`, `best_not_cards`: [..], `top_option`, `above`: ["A", "B"]).
`cargo test --release --test replay` replays every one; it must pass before a commit
(check the exit status, not the printed output). A pure restructuring must also leave the
full analysis of every fixture unchanged (same seed, same JSON): snapshot before and after
with `BAV_SNAPSHOT_DIR=/tmp/before cargo test --release --test replay -- --ignored snapshot`
(then `/tmp/after`) and `diff -r /tmp/before /tmp/after`.
Other local data (never in the repo): `~/.local/share/balatro-advisor/calibration.jsonl`
(predicted vs actual blind results, shops seen).
