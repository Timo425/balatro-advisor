# balatro-advisor: working rules

- **Commit and push** each change once its checks passed (for changes to the advice: step 4 below,
  and the reviewer ran on the fixes). **Ask first** only before heavy dependencies or a change of
  scope (D6).
- **Never commit real game files** (`*.jkr`, snapshots of them, extracted game source). Read them
  in place. Private fixtures go in `tests/fixtures/private/` ("Private fixtures").
- **Ground truth is the game source.** `Balatro.exe` is a zip: `unzip -p
  ~/.steam/debian-installation/steamapps/common/Balatro/Balatro.exe functions/state_events.lua`.
  Cite the file and function in comments; never copy Lua into the repo.
- **Joker facts live in `data/`**, not in logic: `config` numbers from `data/game.json` (`balatro-advisor
  extract-data`). Don't trust the game's `effect` strings.
- **Unmodelled is reported as `not modelled`**, never silently ignored; **heuristics are labelled**
  in every output (text and JSON). Code, comments and docs in English (D4).
- **No coupling** to `balatro-agent` or other local repos: copy and adapt with attribution.
- **Know where things stand before judging.** A chat that brainstorms, judges a gap or picks work
  first runs `tools/status.py`, reads `docs/owner-view.md` (the owner's words only) and says what's
  unknown or stale. What it can find out (a run, a measurement, the history) it finds out; it asks
  the owner only for their judgement, with its own recommendation. A measurement is cited with its
  commit and age.
- **Review the crucial steps, not only code.** Once per task, before the final report, run
  `/fan-out-review` (at least the Opus lead; step 5's skip for docs doesn't apply) on the report,
  the queue cards written from it and any verdict committed to docs, together. Give it the raw results and, as paths it reads itself: the
  queue card, `docs/owner-view.md`, the whole `docs/design.md`, and the owner's history
  (`~/.local/share/balatro-advisor/history/`) when the result is about play. Act on what holds up.
- **Shelve on a missing prerequisite.** When a task turns out to need something not there yet (a
  judge that can't see the change, an engine step not modelled), don't iterate on a proof that
  can't see it: tell the owner in one line, queue the prerequisite ahead of the rest, commit only
  what's proven (the rest named in its gap) and move on.

## Working on the owner's PC

- **The work queue** is `Arvutimängud/Balatro/advisor-queue.md` in the owner's vault (its header
  has the claim rules and the `After:` lines that order cards), not `docs/design.md`'s "Next" line.
- **Shared checkout, no branches.** Other chats commit to main here: commit only your files by name,
  and check `git log` before committing and when a diff looks unexplained.
- **Heavy jobs** (cargo test, the snapshot, the history check, the bench, the shop judge) run one at
  a time per PC across all chats, under `nice -n 19`. When load is high, check your own jobs first
  (a launcher in `ps` doesn't mean the owner is playing).
- **Split runs over ~50 minutes** into parts (`BAV_ONLY`, `BAV_SEEDS`): the prompt cache lasts an
  hour. **Ask before a run that holds the game or the PC for hours**, saying how long; no overnight
  runs (suspending the PC at bedtime is fine).
- **After code changes**, `cargo install --path crates/cli` (the owner restarts `ui`). The bot plays
  only on Profile 3, never the owner's Profile 1.
- **Normal play runs Lovely with only `Mods/advisor-live`.** Never put Steamodded back (it turns
  achievements off); `balatro-agent/tools/mods.sh bot|play` switches.
- **A cloud session's PR** is reviewed and its fixture checks run here, then merged by rebasing onto
  main and a fast-forward push, never a force-push; delete the agent's branches once merged.

## Private fixtures

`tests/fixtures/private/` is its own git repo, the private `Timo425/balatro-advisor-fixtures`, which
every machine pulls and pushes. New machine: `gh repo clone Timo425/balatro-advisor-fixtures
tests/fixtures/private` (a folder that already has fixtures: vault
`Arvutimängud/Balatro/advisor-second-machine.md`). `tools/pull-fixtures.sh` pulls it at each chat's
start (SessionStart hook in `.claude/settings.json`; needs `bash`, Git Bash on Windows) and, in the
background, the run data (history, bot runs, calibration, game file) that the main PC uploads to
Proton every 3 hours. A captured or changed fixture is committed and pushed there in the same sitting
as its advisor commit, its message naming that commit. A fixture named by a claim in
`advisor-active/` is that claim's until it's done. A cloud session has none of this (step 4).

Local data in `~/.local/share/balatro-advisor/` (from the live page): `calibration.jsonl` (predicted
vs actual blinds, every shop: evidence for replacing a register entry), `timings.jsonl`, and
`history/<seed>/<ms>-a<ante>-r<round>-<screen>.jkr` (every state analysed, up to 500 MB / 90 days):
capture reported advice from there (`state --file <jkr> --json`; `analyze --file <jkr>` replays it).

**"What if I…" questions:** use `balatro-advisor whatif` (not scratch programs): a named plan
against the blinds ahead, next to the board as it is; `--help` for the flags (`--jokers a,b,b`,
`--sell`, `--add`, `--card "OLD=NEW"`, `--add-cards`, `--remove-cards`, `--json`). Spotting the line
is your part.

## Refining the advice (required workflow)

`docs/design.md` is the architecture: the four stages every piece of advice goes through (moves
tried, simulated player, valuation, noise), the patterns and rules, the register of hand-set values
and the retire list. Read it before changing what the advice says. Every change to how the advice
decides or values things follows these steps, in order:

1. **Capture.** When the owner says a suggestion is wrong, save that state as a replay fixture before
   touching code: `tests/fixtures/private/<name>.json` = `{"note", "state": <balatro-advisor state
   --json>, "expect": {...}}` (keys: `best_action`, `best_hand`, `best_use_first`, `best_size`,
   `best_not_cards`: [..], `best_not_played`: [..], `best_played`: [..], `top_option`, `above`:
   ["A", "B"], `rank_above`: ["A", "B"] (by value, not tie-band order), `top_hand`, `share_below`:
   {"Joker": x}, `sells`: {"j_key": "Joker"}), committed in the fixtures repo (D18). When a known gap
   stops a fix meeting an expectation, narrow `expect` to what holds and move the rest to
   `"pending_expect"` with `"pending_gap": "gap N"` (the gap names the fixture; the note says why).
   The replay test fails once it holds, so the fix closing the gap restores it (D14).
2. **Diagnose before fixing.** Find out why by reading the code and simulating the state (`whatif`
   or scratch programs), not by guessing. Name the stage that failed and run `git log --oneline |
   grep -i '<stage>'` (commits before d8c27ef aren't prefixed: read the recent log too); if it was
   fixed before, change how the stage works for every case instead of adding a rule to it.
3. **Fix the stage, generally.** Ask what the case is an example of and where the game (or the
   simulation) already defines that group. A fix that only makes sense for one joker, suit, hand or
   card is the wrong fix. New hand-set numbers go in the register.
4. **Prove it**, in two tiers (D12). Always:
   - `cargo test` (not `--release`: the test profile is optimised and keeps overflow checks, D13)
     exits 0 (check the exit status), replay fixtures included; `cargo clippy --release
     --all-targets` reports no warnings from your change.
   - Snapshot diff: `BAV_SNAPSHOT_DIR=/tmp/before cargo test --release --test replay -- --ignored
     snapshot` before, `/tmp/after` after, `diff -r`. A pure restructuring leaves it unchanged; a
     behaviour change explains every difference.
   - History check: `tests/history_picks.rs` before (in a `git worktree` of the base commit, started
     while you work) and after (~15 min each, the same 350 saved states, same UTC day), then
     `history_judge` in `tests/history.rs` (each moved pick against the old one, the oracle playing
     on). Explain or fix every WORSE, unclear, ? and LOST pick (the
     judge shares the valuation and engine: a change to those half judges itself, so say so).

   Where it applies:
   - Simulated player or simulated game (the engine, what a round simulates): `cargo test --release
     --test player -- --ignored --nocapture` before and after: win rates (explain every board that
     gets worse) and the gap to the oracle on the boards touched (D9; after a change to the game only
     the gap judges the player).
   - Engine (`engine/`) or a `Board` step a simulated round plays on (`after_hand`, `discard`,
     `at_blind_set`), and after a game update: the parity tier (D17), `BAV_PARITY_DIR=/tmp/before
     nice -n 19 cargo test --release --test parity -- --ignored parity --nocapture` (~15 min) before
     and after, then `diff` the two `summary.txt`: every new difference explained or fixed.
   - Best play's search or the noise stage: `cargo test --release --test replay -- --ignored
     search_against_reference --nocapture` before and after (slow: split with `BAV_ONLY`/`BAV_SEEDS`),
     judged by how close each pick is to the best, not by which fixture flips; other changes run it on
     the fixtures whose Best play moved in the snapshot diff.
   - Without the fixtures and the history (a cloud session), prove values with tests that build the
     state in code and say in the PR which checks weren't run; they run here before merging.
5. **Review.** Spawn the `advisor-reviewer` agent on the change (it hasn't seen your reasoning).
   Check its claims in the code, apply what holds up, then run it once more on the fixes. Skip only
   for changes that don't touch how advice is decided or valued. A change touching many places also
   gets 4–8 Haiku checkers with the first review (same agent, `model: haiku`, effort high), each with
   ONE bounded check and the files it may read (game fidelity, callers reached, determinism, edge
   cases, tests that fail without the change, docs and labels); findings need file:line, a failing
   scenario, severity, confidence and evidence, and each is verified before acting (D11).
6. **Record.** The commit message starts with the stage (`valuation: …`, `simulated player: …`,
   `moves tried: …`, `noise: …`; `refactor`/`docs` otherwise). Update `docs/design.md` when a stage,
   rule, register entry or known gap changed, and its "Where things stand" lines when the work moves
   on. Then commit and push.

**When the rules don't fit the fix.** First ask whether the fix brings the advice closer to the
question every piece of advice answers (`docs/design.md`, top: a choice is worth its chance to get
through, times the long-run value of what it leaves you). If so, it carries out the design. Only when
a correct fix makes a sentence of the stages, patterns or numbered rules untrue, quote it and say why
it can't stay true; no silent exception in code or docs. Then:
- **For now** (keeping it true needs work not there yet): name the bend in its known gap with the
  quoted sentence and what replaces it (a queue card), and label it in the output if it's a
  heuristic. No entry; the owner isn't asked.
- **For good** (the rule should change): widen or replace it in `docs/design.md`, explained by how it
  serves that question and which other situations it covers, and record it in `docs/decisions.md`
  (what, why, what would make us revisit). A rule only one case needs, or an exception, is rejected;
  a rule change without an entry doesn't count, and the reviewer flags it.

**What needs a decision.** A `docs/decisions.md` entry is needed only when a change makes a sentence
untrue in `docs/design.md`'s stages, patterns or numbered rules, in this workflow, or in a
project-wide choice (language, dependencies, scope): quote the sentence. With none, it's
`docs/design.md` work however large (carrying out a rule, a stage, a register value, a retire-list
item, a known gap). A change to an existing decision amends it in place, marked with the date. This
is the one definition; the reviewer and `docs/decisions.md` point here.

The owner approves only scope, new dependencies, what the advice should value in play, and this
workflow; a technical rule changed for good gets one line in the final report, in game terms.

**Example.** The simulated round spent a 3♠ Blue Seal in a Flush. Wrong: "keep Blue Seal cards in
hand while on pace" (named after the card on screen). Right: "cards that pay at the round's end while
held", a group the game defines (card.lua `get_end_of_round_effect`), so Gold cards were covered too.
