# balatro-advisor: design

How the advice is built, and how to change it. Read this before changing what the advice
says. The required workflow for a change (fixture, diagnosis, fix, proof, review, record) is
in `CLAUDE.md`; the original plan is `plan.md`.

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
| **Moves tried** | Which choices are compared? All of them, narrowed down by simulation, never a hand-picked few. | Best play (`advise/play.rs`): `sim::all_plays` (every 1–5 card play) and `sim::all_discards` (every discard), screened in stages, for your hand as it is and after each held consumable. Shop: `advise.rs` `shop_options` (every shop item, pack pick and "leave"). Skip-or-play. |
| **Simulated player** | How is the rest of the round played out? Toward the same measure the advice uses. | `sim::decide` (play/discard policy, a labelled heuristic), `sim::RoundGoals` + `play_on_instead` (weighs "win now" against playing on, with a few simulated futures), `sim::pays_at_end` (cards that pay at round end stay in hand while on pace), `sim::use_if_better` (held consumables) |
| **Valuation** | What is an outcome worth? One measure, measured by simulating your board. | `advise/value.rs`: `LongRun` (your board projected to Ante 8; `value(&Gain)` for anything a run gains: one-off money, money held, planets with Constellation, levels; `planet(hand)`; `long_of` for jokers), `Spending` (money held past the planet-level cap, worth the rerolls and packs it buys). `RoundGoals` for the simulated player is built from these. Shop ranking: `rank_options` (survive × next-ante survival × long-run). |
| **Noise** | Which differences are real? | `advise/compare.rs` `race`: the same draws for every option, in batches until the best is clear; options within 1% of the leader's value are ties, reported as ties. Same seeds (common random numbers) everywhere; cards are put in one fixed order (`Card::order_key`) before anything random touches them, so card order never changes the advice (tested). |

Layers underneath: `save` (reads the game's files), `engine` (the scoring pass, checked
against real in-game scores in the golden tests), `sim` (rounds), `advise` (the four stages
above, and the output).

## Patterns

The engineering practices behind the stages, in plain words:

- **One source of truth.** Each idea is defined once and reused: what a run gains (`Gain`),
  what it's worth (`LongRun::value`), a planet (`LongRun::planet`), held money (`Spending`),
  how options are compared (`compare::race`). Two formulas for one thing will drift apart.
- **A pipeline of stages.** Moves tried → simulated player → valuation → noise. Each piece of
  code belongs to one stage and each fix to one stage.
- **Search, then narrow.** Generate every candidate and let simulation discard the bad ones;
  never rely on a list of moves someone thought of.
- **Assumptions are parameters.** A number nobody can derive is a named value in one place,
  listed in the register, labelled in the output, and replaceable by a measurement.
- **Deterministic.** Same input, same output: fixed seeds, and nothing depends on the order
  cards or options happen to be listed in.
- **Regression-proofed.** Every state where the advice was caught wrong becomes a replay
  fixture; restructurings must reproduce the output exactly; behaviour changes are reviewed as
  differences.
- **Fresh eyes.** A reviewer that hasn't seen the reasoning (`advisor-reviewer`) checks every
  change to how advice is decided or valued, and checks the fixes to its findings.

## Rules for changing the advice

1. **One measure.** Every value of money, planets, jokers or cards goes through
   `advise/value.rs` (`Gain` → `LongRun::value`, `LongRun::planet`, `Spending`). A new kind
   of gain is a new field in `Gain`, not a new formula somewhere else.
2. **Search, don't pick.** A new kind of decision generates all its candidates and lets
   simulation narrow them down.
3. **The simulated player shares the goal.** Anything worth having beyond winning goes into
   `RoundGoals` and `sim::Outcome`, so the advice and the simulated player both see it. When
   the advice misses a line, first ask whether the simulated player would ever play it.
4. **Groups the game defines, not cards.** When something should apply to a card, ask what
   the card is an example of and where the game defines that group (cite the Lua file and
   function), e.g. "cards that pay at the end of the round" (`card.lua`
   `get_end_of_round_effect`: Blue Seal, Gold card).
5. **Card-specific code only as game facts, only in the engine and `data/`.** A joker's own
   effect belongs in the engine (scoring) or `data/`. Moves tried, the simulated player and
   valuation stay general: no branches on a joker, card or suit name there. Existing ones are
   on the retire list below.
6. **Noise is handled by simulation, not by side numbers.** Compare on the same draws,
   simulate more where options are close, report ties.
7. **Every hand-set number is in the register below,** with what it stands for, and labelled
   in the output as a heuristic. Replacing one with a measurement is the best kind of change.
8. **Prove and review** (`CLAUDE.md` workflow): fixture first; tests and replay exit 0;
   snapshots identical for a restructuring, differences explained for a behaviour change;
   `advisor-reviewer` on the change and on the fixes.

## Assumption register

Hand-set values, where they live, what they stand for, and how each could be measured
instead (most from `~/.local/share/balatro-advisor/calibration.jsonl`, which logs predicted
vs actual blind results and every shop seen).

| Value | Where | Stands for | Could be measured by |
|---|---|---|---|
| Typical find ×1.25; empty slots ×1.5 / +60 Chips / +15 Mult by shop weights | `value.rs` `stand_in`, `fill_long` | the jokers you'll find by Ante 8 | final boards of won runs (calibration log) |
| Money worth 30·(1 − e^(−m/30)) | `value.rs` `spendable` | money buys less the more you have | what money turned into in logged shops |
| Planet levels from money held: 0.5 an ante + (money − interest line)/20, at most 2 an ante; $18 an ante below the line | `value.rs` `levels_for` | planets bought with spare money | logged planet buys per ante |
| One-off money: $5 a reroll or pack skip, $12 a main-hand level, $4 any planet (the same planets both level and grow Constellation) | `value.rs` `spend_once` | what a one-off sum buys | logged shops |
| Rent $9 an ante | `value.rs` `RENT_PER_ANTE` | $3 a round × 3 | exact |
| 48 rounds per projected board | `value.rs` `long_score` | projection noise | compare against more rounds |
| Growth by Ante 8: at most 6 growth steps bought with money, 12 hands an ante, per-joker rates | `advise.rs` `grow_antes` | how growing jokers grow | logged runs |
| Madness horizon 1 ante | `value.rs` `long_of` | it eats your jokers | — |
| Spending: level cap reached at the interest line + $30; packs $4, 2 a shop, each further one ×0.8; typical pack by shop weights (game.lua 4/4/1.2/0.6/4), Standard counted as 0 | `value.rs` `Spending` | money past the cap buys rerolls and packs | logged packs opened and picks |
| Rerolls: up to 8 a shop; each further joker bought counts ×0.6 | `advise.rs` `money_value_with`, `expected_buys` | joker gains don't simply add up | logged rerolls |
| A dollar in Best play: what +$10 does, taken as linear | `play.rs` `dollar_gain` | money won this round | — |
| Comparison: first batch 64 rounds, at most 1,600, "equal" within 1% of the leader's value | `compare.rs` | when a difference is real enough to act on | — |
| Move screening: every play and discard on 16 rounds, best 48 on 64, best 16 on 256, best 6 into the comparison; draws of its own; a cut drops only moves clearly worse than the stage leader; value ties broken by points toward the target | `advise.rs` `SCREEN_STAGES`, `play.rs` `screen` | cost vs. coverage | check if better moves are ever missed |
| Simulated player's policy: flush chase when odds > 10%, 1.5× better than the best hand when on pace; Mail-In and Mystic Summit rules | `sim.rs` `decide_cards` | how a player digs | replay fixtures; calibration of win chances |
| Look-ahead: 8 futures, only when a better finish could add ≥ 1% in seal planets | `sim.rs` `play_on_instead`, `LOOKAHEAD_ROLLOUTS` | cost vs. depth | replay fixtures |
| Consumable used in a simulated round when it lifts the best play by > 1% | `sim.rs` `use_if_better` | when a player uses one | — |
| Blue Seal: drawn with the hand plus 3 cards an action; Mail-In rank 4 in 13 | `advise.rs` `seal_round_chance`, Mail-In income | how often a card shows up | game source + logged rounds |
| Glass cards: about 1.5 scores an ante before breaking | `advise.rs` `glass_presence` | Glass breaking | — |
| Deck changes (tarots): 300 rounds against your deck; `CHIP_JOKERS` list | `advise.rs` `deck_long`, `TAROT_ROUNDS` | a separate baseline from `LongRun` | (should move into `value.rs`) |
| Vouchers: Overstock half a reroll a shop, Reroll Surplus $2 a shop, Clearance 25% of ~$8, Hone one buy an ante, a voucher shows 1 in 16, Planet Merchant 1 in 12 on your main hand | `advise.rs` economy and voucher values | what each voucher saves | logged shops |
| Tags: Meteor's main-hand planet 5 in 12, best 1 of the pick 2; Investment not discounted for arriving later; Mega pack's second pick $2.50; Emperor average² | `advise.rs` skip-or-play, packs, tarots | — | logged tags and packs |
| Skip-or-play "close": within 3% | `advise.rs` skip-or-play | noise | (should become `compare.rs`) |
| Shop ranking: ties within 3%; under 20% this round, score reach decides; next-ante survival dropped when under 1% for every option; a sell choice may cost at most 5 points this ante | `advise.rs` `rank_options`, `NOW_SLACK` | noise and hopeless rounds | (should become `compare.rs`) |
| Perishable gone by Ante 8 if its rounds < 3 × antes left; Gros Michel breaks 1 in 6 a round | `value.rs` `lasts`, `advise.rs` | — | game source (exact) |

## Retire list

Narrow rules still in the code, each to be replaced by its stage's general mechanism. Don't
add to this list's kind; remove from it.

| Rule | Where | General replacement |
|---|---|---|
| Mystic Summit: discard first, and its tip | `sim.rs` `decide_cards`, `play.rs` tip | the simulated round values what it ends with (gap 2) |
| Mail-In: cash its rank first | `sim.rs` `decide_cards` | the same: discard money counted in the round's value, choices weighed by it |
| Holding Cryptid: drawing a Blue Seal is worth $15 (also inflates the round money shown) | `play.rs`, `sim.rs` `seal_seen_value` | the round's end state (deck, consumables) valued by `LongRun` |
| The Lovers makes a pack card Wild | `advise.rs` standard pack cards | consumables used on pack cards, searched |
| Tarot targets by a fixed priority (Blue Seal, then Red/Glass, then main suit); two other "best card" orderings (DNA, Cryptid tip) | `advise.rs` `tarot_values`, `value.rs` `dna_long` | search every target in hand |
| Suit-joker map, `kickers_matter` list, `income_per_ante` per joker | `sim.rs` `keep_suit`, `advise.rs` | game facts: move to `data/` or the engine; or work out what to keep by scoring with and without a card |
| Red Card grows in three places | `grow_antes`, `spend_once`, skip-or-play | one place, through `Gain` |
| A short perishable's price dropped from its value | `advise.rs` (`long_mult = 1 + unlock`) | `LongRun::value` with its price |

## Known gaps

Ranked by how likely each is to cause the next round of patch-on-patch.

1. **Tarot targets aren't searched** (see the retire list).
2. **The simulated round only reports planets, money and hands left.** It should report the
   board, deck and consumables it ends with, valued by `LongRun`. That would retire the
   Cryptid, Mystic Summit and Mail-In rules, and let the look-ahead weigh more than seal planets.
3. **Valuation is still split.** `deck_long` (tarot deck changes), the voucher, tag and Emperor
   formulas, and the shop ranking spend leftover money on rerolls in each of its three
   factors. The general fix: deck, joker and hand-size fields in `Gain`, and each dollar spent
   once in one model.
4. **The simulated player's policy (`decide_cards`) is a rulebook,** partly keyed to jokers, and
   only ever chases flushes. The general fix: compare policy moves with a cheap search using
   the engine, as the look-ahead does at "win now".
5. **Shop ranking and skip-or-play aren't confidence-based.** They tie within 3% instead of
   using `compare.rs`, because their options are valued by separate simulations, not round by
   round on the same draws.
6. **The long-run projection spends money on your main hand's planets.** A board that wants
   several hands levelled is valued as if it wanted one.
7. **`advise.rs` still holds the shop options, tarot valuation and outlook inline,** and the
   outlook's heuristics note says growth isn't projected, which is out of date (`grow_antes`
   projects it).
