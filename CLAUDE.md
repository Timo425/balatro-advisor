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

## Refining the advice (required workflow)

`docs/design.md` is the architecture: the four stages every piece of advice goes through
(moves tried, simulated player, valuation, noise), the patterns and rules, the register of
hand-set values and the retire list. Read it before changing what the advice says. Every
change to how the advice decides or values things follows these steps, in order:

1. **Capture.** When the owner says a suggestion is wrong, save that state as a replay
   fixture before touching code: `tests/fixtures/private/<name>.json` = `{"note", "state":
   <balatro-advisor state --json>, "expect": {...}}` (keys: `best_action`, `best_hand`,
   `best_use_first`, `best_not_cards`: [..], `top_option`, `above`: ["A", "B"]). The folder is
   git-ignored (real runs aren't committed).
2. **Diagnose before fixing.** Find out why, by reading the code and simulating the state
   (scratch programs or `whatif`), not by guessing. Name the stage that failed. Run
   `git log --oneline | grep -i '<stage>'` for earlier fixes to it (commits before d8c27ef
   aren't prefixed: read the recent log too); if there are any, change how the stage works
   for every case rather than adding another rule to it.
3. **Fix the stage, generally.** Ask what the case is an example of and where the game (or
   the simulation) already defines that group. If the fix only makes sense for one joker,
   suit, hand or card, it's the wrong fix. New hand-set numbers go in the register.
4. **Prove it.** `cargo test --release` must exit 0 (check the exit status, not the printed
   output), the replay fixtures included. A pure restructuring must leave the full analysis
   of every fixture unchanged: `BAV_SNAPSHOT_DIR=/tmp/before cargo test --release --test
   replay -- --ignored snapshot` before and `/tmp/after` after, then `diff -r`. A behaviour
   change: diff the snapshots and explain every difference.
5. **Review.** Spawn the `advisor-reviewer` agent (`.claude/agents/`) on the change. It hasn't
   seen your reasoning, which is why it finds what you missed. Apply what holds up (check its
   claims in the code first; it can be wrong), then run it once more on the fixes. Skip only
   for changes that don't touch how advice is decided or valued (UI text, docs).
6. **Record.** Commit message starts with the stage (`valuation: …`, `simulated player: …`,
   `moves tried: …`, `noise: …`; `refactor`/`docs` otherwise). Update `docs/design.md` when a
   stage, rule, register entry or known gap changed. Then commit and push.

**When the rules don't fit the fix.** Sometimes a correct fix can't be made within
`docs/design.md` (it needs a new kind of value, a new stage, or breaks a rule). Then:
1. Stop and say which rule doesn't fit and why. No silent exception in code or docs.
2. Propose a change to the design itself: a new or wider rule, pattern, `Gain` field or
   stage, explained by how it serves the question every piece of advice answers ("most
   likely to win, and stronger") and which other situations it covers.
3. Accept it only if it's general: it widens or replaces a rule. A rule that only one case
   needs, or an exception to a rule, is a patch in the docs and is rejected.
4. The owner approves; record it in `docs/decisions.md` (next `D` number: what, why, what
   would make us revisit). Rule changes without an entry don't count, and the reviewer
   flags them.

**Example.** The advice played a junk Spade instead of digging with the off-suit cards,
because the simulated rest of the round spent the 3♠ Blue Seal in a Flush.
- First fix (wrong scope): "keep Blue Seal cards in hand while on pace". Right stage (the
  simulated player), but named after the card on screen.
- Right fix: "cards that pay at the end of the round while held". The game defines that group
  (card.lua `get_end_of_round_effect`: Blue Seal, Gold card), so the policy keeps whatever is
  in it, and Gold cards were covered without anyone noticing them.

Other local data (never in the repo): `~/.local/share/balatro-advisor/calibration.jsonl`
(predicted vs actual blind results, every shop seen): the evidence for replacing a register
entry with a measurement.
