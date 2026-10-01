# balatro-advisor: design

How the advice is built, and how to change it. Read this before changing what the advice
says. The rules for working in the repo are in `CLAUDE.md`; the original plan is `plan.md`.

## The question every piece of advice answers

Which choice leaves the run most likely to win, and stronger? Winning the round is the
entry point, not the goal: a choice is worth its chance to get through, times the long-run
value of what it leaves you (planets, money, jokers, deck). The same measure ranks shop
options, skip-or-play and the best play, and the simulated player plays toward it.

## Four stages

Every piece of advice goes through the same four stages. When advice is wrong, one of them
failed: fix that stage for every case, not the case.

| Stage | Question | Where |
|---|---|---|
| **Moves tried** | Which choices are compared? All of them, narrowed down by simulation, never a hand-picked few. | `sim::candidate_moves` (plays: every subset, best kept), `sim::all_discards` (every discard, screened in stages), `advise/play.rs` (each held consumable used first), `advise.rs` `shop_options` (every shop item, pack pick and "leave"), skip-or-play |
| **Simulated player** | How is the rest of the round played out? Toward the same measure the advice uses. | `sim::decide` (play/discard policy, a labelled heuristic), `sim::RoundGoals` + `play_on_instead` (weighs "win now" against playing on, with a few simulated futures), `sim::pays_at_end` (cards that pay at round end stay in hand while on pace), `sim::use_if_better` (held consumables) |
| **Valuation** | What is an outcome worth? One measure, measured by simulating your board. | `advise/value.rs`: `LongRun` (your board projected to Ante 8; `value(&Gain)` for anything a run gains: one-off money, money held, planets with Constellation, levels; `planet(hand)`; `long_of` for jokers), `Spending` (money held past the planet-level cap, worth the rerolls and packs it buys). `RoundGoals` for the simulated player is built from these. Shop ranking: `rank_options` (survive × next-ante survival × long-run). |
| **Noise** | Which differences are real? | `advise/compare.rs` `race`: the same draws for every option, in batches until the best is clear; options that can't be told apart are ties, reported as ties. Same seeds (common random numbers) everywhere. |

Layers underneath: `save` (reads the game's files), `engine` (the scoring pass, checked
against real in-game scores in the golden tests), `sim` (rounds), `advise` (the four stages
above, and the output).

## Rules for changing the advice

1. **One measure.** Every value of money, planets, jokers or cards goes through
   `advise/value.rs` (`Gain` → `LongRun::value`, `LongRun::planet`, `Spending`). A new kind
   of gain is a new field in `Gain`, not a new formula somewhere else.
2. **Search, don't pick.** A new kind of decision generates all its candidates and lets
   simulation narrow them down. A hand-written candidate list means some good move depends on
   someone having thought of it.
3. **The simulated player shares the goal.** Anything worth having beyond winning goes into
   `RoundGoals` and `sim::Outcome`, so the advice and the simulated player both see it. When
   the advice misses a line, first ask whether the simulated player would ever play it.
4. **Groups the game defines, not cards.** When something should apply to a card, ask what
   the card is an example of and where the game defines that group (cite the Lua file and
   function), e.g. "cards that pay at the end of the round" (`card.lua`
   `get_end_of_round_effect`: Blue Seal, Gold card).
5. **Noise is handled by simulation, not by side numbers.** Compare on the same draws,
   simulate more where options are close, report ties.
6. **Every hand-set number is in the register below,** with what it stands for, and labelled
   in the output as a heuristic. Replacing one with a measurement is the best kind of change.
7. **Prove it.** The state that showed the problem becomes a replay fixture first; a pure
   restructuring must reproduce the analysis exactly (same seed, same output); behaviour
   changes are reviewed as differences in the replay states. Details in `CLAUDE.md`.

## Assumption register

Hand-set values, where they live, what they stand for, and how each could be measured
instead (most from `~/.local/share/balatro-advisor/calibration.jsonl`, which logs predicted
vs actual blind results and every shop seen).

| Value | Where | Stands for | Could be measured by |
|---|---|---|---|
| Typical find ×1.25; empty slots ×1.5 / +60 Chips / +15 Mult by shop weights | `value.rs` `stand_in`, `fill_long` | the jokers you'll find by Ante 8 | final boards of won runs (calibration log) |
| Money worth 30·(1 − e^(−m/30)) | `value.rs` `spendable` | money buys less the more you have | what money turned into in logged shops |
| Planet levels from money held: 0.5 an ante + (money − interest line)/20, at most 2 an ante; $18 an ante below the line | `value.rs` `levels_for` | planets bought with spare money | logged planet buys per ante |
| One-off money: $5 a reroll or pack skip, $12 a main-hand level, $4 any planet | `value.rs` `spend_once` | what a one-off sum buys | logged shops |
| Rent $9 an ante | `value.rs` `RENT_PER_ANTE` | $3 a round × 3 | exact |
| 48 rounds per projected board | `value.rs` `long_score` | projection noise | compare against more rounds |
| Spending: level cap reached at the interest line + $30; packs $4, 2 a shop, each further one ×0.8; typical pack by shop weights (game.lua 4/4/1.2/0.6/4), Standard counted as 0 | `value.rs` `Spending` | money past the cap buys rerolls and packs | logged packs opened and picks |
| Rerolls: up to 8 a shop; each further joker bought counts ×0.6 | `advise.rs` `money_value_with`, `expected_buys` | joker gains don't simply add up | logged rerolls |
| Comparison: first batch 64 rounds, at most 1,600, "equal" within 1% of the value | `compare.rs` | when a difference is real enough to act on | — |
| Discard screening: every discard 32 rounds, best 24 on 160, best 6 into the comparison | `advise.rs` `SCREEN_DISCARD_STAGES` | cost vs. coverage | check if better discards are ever missed |
| Simulated player's policy: flush chase when odds > 10%, 1.5× better than the best hand when on pace; Mail-In and Mystic Summit rules | `sim.rs` `decide_cards` | how a player digs | replay fixtures; calibration of win chances |
| Look-ahead: 8 futures, only when a better finish could add ≥ 1% | `sim.rs` `play_on_instead`, `LOOKAHEAD_ROLLOUTS` | cost vs. depth | replay fixtures |
| Consumable used in a simulated round when it lifts the best play by > 1% | `sim.rs` `use_if_better` | when a player uses one | — |
| Shop ranking: ties within 3%; under 20% this round, score reach decides; next-ante survival dropped when under 1% for every option; a sell choice may cost at most 5 points this ante | `advise.rs` `rank_options`, `NOW_SLACK` | noise and hopeless rounds | — (should become `compare.rs`) |
| Perishable gone by Ante 8 if its rounds < 3 × antes left; Gros Michel breaks 1 in 6 a round | `value.rs` `lasts`, `advise.rs` | — | game source (exact) |
| Holding Cryptid without a Blue Seal in hand: drawing one is worth $15 | `advise/play.rs` (`seal_seen_value`) | a legacy special case | should become a `RoundGoals` value |

## Known gaps

- **Shop ranking isn't confidence-based yet.** It ties options within 3% instead of using
  `compare.rs`: its options are valued by separate simulations, not round by round on the
  same draws.
- **The long-run projection spends money on your main hand's planets.** A board that wants
  several hands levelled is valued as if it wanted one.
- **The simulated player looks ahead only at "win now" moments.** Other decisions (which
  discard, which dig) follow `decide`'s heuristics.
- **`advise.rs` still holds the shop options and tarot valuation inline** (`shop_options`,
  `tarot_values`, the tarot and pack parts of `analyze`): the next things to move into their
  stage's module.
