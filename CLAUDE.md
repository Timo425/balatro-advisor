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
