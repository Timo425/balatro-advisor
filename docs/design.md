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
| **Moves tried** | Which choices are compared? All of them, narrowed down by simulation, never a hand-picked few. | Best play (`advise/play.rs`): `sim::all_plays` (every 1–5 card play) and `sim::all_discards` (every discard), for your hand as it is and after each held consumable, all in one `compare::race`. Shop: `advise.rs` `shop_options` (every shop item, pack pick and "leave"). Skip-or-play. A consumable's targets: every set of cards its game data allows (`engine::consumable`), in the hand it's used from, in one `compare::race` on the deck projection. |
| **Simulated player** | How is the rest of the round played out? Toward the same measure the advice uses. | `sim::decide` (play/discard policy, a labelled heuristic), `sim::RoundGoals` + `play_on_instead` (weighs "win now" against playing on, with a few simulated futures), `sim::pays_at_end` (cards that pay at round end stay in hand while on pace), held consumables: used for score only when the round needs it (`sim::use_if_better`, off pace), and right before the winning hand when that makes the win worth more (`sim::finish_with_uses`: a Blue Seal or Gold card more in hand at the end, a slot freed for a planet) |
| **Valuation** | What is an outcome worth? One measure, measured by simulating your board. | `advise/value.rs`: `LongRun` (your board projected to Ante 8; `value(&Gain)` for anything a run gains: one-off money, money held, planets with Constellation, levels; `planet(hand)`; `long_of` for jokers), `Spending` (money held past the planet-level cap, worth the rerolls and packs it buys). `RoundGoals` for the simulated player is built from these. Shop ranking: `rank_options` (survive × next-ante survival × long-run). |
| **Noise** | Which differences are real? | `advise/compare.rs` `race`: the same draws for every option, in batches on fresh rounds until the best is clear; options clearly worse drop, options within 1% of the leader's value are ties, reported as ties (a tie takes at least ``TIE_ROUNDS`` shared rounds; a tie stands only through ties to the final leader; tied moves are ordered by their paired difference to the leader); past that, an explicit budget (by value, then a secondary value such as how close lost rounds came to the target) limits how many stay in, and is named as a budget, not a finding. The search is measured against a reference (every move on `compare::MAX` rounds of its own: `search_against_reference` in tests/replay.rs). Same seeds (common random numbers) everywhere; cards are put in one fixed order (`Card::order_key`) before anything random touches them, so card order never changes the advice (tested). |

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
| Comparison: at most 1,600 rounds an option, "clearly worse" at 95% (paired), "equal" within 1% of the leader's value | `compare.rs` `MAX`, `EQUAL` | when a difference is real enough to act on | — |
| Best play's search budget: first batch 16 rounds (batches double); at most 48 undecided moves after 16 rounds, 24 after 32, 12 after 64, 8 after that, kept by value then how close lost rounds came to the target | `advise.rs` `SEARCH_FIRST`, `SEARCH_BUDGET` | cost vs. coverage | `search_against_reference`: on the fixtures (2026-10-03) every pick is within 1% of the best move |
| A tie (two options within 1%) takes at least 300 shared rounds: a round where they part ways can be worth a whole round's value, and when none has turned up in n rounds it may still happen 3 in n (95%, the rule of three) | `compare.rs` `TIE_ROUNDS` | the rule of three | — |
| Which hands carry your points (the main hand planets go to, joker growth, the style outlook): the hardest round of the ante, played from the start (a round in progress begins again) | `advise.rs` `hand_mix`, `fresh_round` | your hands in a typical round of that blind | — |
| Simulated player's policy: flush chase when odds > 10%, 1.5× better than the best hand when on pace; discards that pay money (`engine::discard_money`) cashed while on pace (a threshold, not yet weighed against `RoundGoals` money), the best-paying one whose remaining hand keeps pace, at most 16 tried, the last one right before the round ends; discards burned first when the chosen play scores > 0.1% more with none left (`burn_pays`; which cards, the policy's own dig choice) | `sim.rs` `decide_cards` | how a player digs | replay fixtures; calibration of win chances |
| Cards worth drawing this round: for each held consumable that goes on one card (one that takes more isn't counted: a race per card kind costs too much, so a pack card picked for an Empress isn't "worth drawing" later), its value on each kind of card in the draw pile minus its value on the target its search chose in hand (48-round screen, kept when over 1% on the full projection; Blue Seal planets counted); tied to its consumable and gone once that's used; a simulated round counts the best one it draws | `play.rs` `seen`, `sim::RoundGoals::seen` | a better target may come | — |
| A consumable's targets (every one that goes on cards you pick): every target set in the hand it's used from (yours when it's held or in the open pack and a hand is on screen, else 4 sampled hands for one you can buy, pick or hold, 1 for the rest of the pool: noisy, and in a blind it reaches Best play's money value through pack values), plus no target (your deck as it is), sets leaving the same deck counted once, chosen by `compare::race` on the deck projection (first batch 16 rounds; at most 12 sets undecided after 16, 6 after 32, 3 after that; at most 128 rounds, however many sets a big hand gives; the money a set's cards earn becomes income once, from its first batch, against your deck's money on the same rounds), on rounds of their own (after the 300 the pick is then valued on); sets as good as the best are listed; a deck's extra Blue Seal cards counted as planets (drawn with `seal_round_chance`, 3 rounds an ante) | `advise.rs` `TARGET_FIRST`, `TARGET_MAX`, `TARGET_BUDGET`, `value.rs` `deck_rounds`, `deck_value` | cost vs. accuracy (a first batch of 8 dropped the best targets) | check if better targets are ever cut by the budget |
| Random effects (Immolate, Familiar, Grim, Incantation, Aura, Sigil) and the shop's sampled hands: valued over several outcomes (6 for the destroyers), the projection's 300 rounds split between them, each outcome on rounds of its own (`deck_value_part`); this round's odds for the destroyers come from one outcome | `advise.rs` `RANDOM_OUTCOMES` | the average outcome | — |
| Typical hands with jokers that pay by discards left: scored with your discards and with none, the better counts (only when a reference Pair scores differently) | `advise.rs` `Ctx::typical_n` | you'd keep or burn them | — |
| Look-ahead: 8 futures, only when a better finish could add ≥ 1% in seal planets | `sim.rs` `play_on_instead`, `LOOKAHEAD_ROLLOUTS` | cost vs. depth | replay fixtures |
| Consumable used in a simulated round when it lifts the best play by > 1% | `sim.rs` `use_if_better` | when a player uses one | — |
| Blue Seal: drawn with the hand plus 3 cards an action; Mail-In rank 4 in 13 | `advise.rs` `seal_round_chance`, Mail-In income | how often a card shows up | game source + logged rounds |
| Glass cards: about 1.5 scores an ante before breaking; what a deck's extra Glass cards add (against the deck with them destroyed, as breaking does) counts for the share of the antes left they last, the rest of the change in full | `advise.rs` `glass_presence`, `value.rs` `without_new_glass` | Glass breaking | — |
| A Standard pack card with a consumable you hold on it: for every held one that goes on cards (`engine::consumable`), one race (`target_race`) over a hand you'd hold with the card in it (the first hand its own value is sampled on, one card swapped for it); credited only when the best targets include the card and nothing as good leaves it out, as the card × (with it used on the card) / (with it used on the best set without it), paired on the same rounds; labelled "assuming you draw it while you still hold" it | `advise.rs` standard pack cards, `target_race` | you'll draw it and keep the consumable for it | the chance to draw it before you'd use the consumable elsewhere (`seal_round_chance`) |
| Deck changes (tarots, pack cards): 300 rounds against your deck (split between outcomes for random effects); a board without a chips joker (`CHIP_JOKERS` list) gets a +60 Chips find | `value.rs` `deck_stats`, `TAROT_ROUNDS` | deck changes are small; a chips gap closes by Ante 8 | — |
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
| Two "best card" orderings: DNA's copies, Aura's target (a random edition, so not in `engine::consumable`) | `value.rs` `dna_long`, `advise.rs` `tarot_values` | the target search (`engine::consumable` + `compare::race`), with random effects over their outcomes |
| Suit-joker map, `kickers_matter` list, `income_per_ante` per joker | `sim.rs` `keep_suit`, `advise.rs` | game facts: move to `data/` or the engine; or work out what to keep by scoring with and without a card |
| Red Card grows in three places | `grow_antes`, `spend_once`, skip-or-play | one place, through `Gain` |
| A short perishable's price dropped from its value | `advise.rs` (`long_mult = 1 + unlock`) | `LongRun::value` with its price |

## Known gaps

Ranked by how likely each is to cause the next round of patch-on-patch.

1. **Best play and the simulated round try one target per consumable** (a simulated round
   that draws a better target doesn't switch to it), the one its search chose by the Ante 8
   projection (ties listed, not weighed by this round), as its "use now" move. Targets that win this
   blind but leave a weaker deck aren't tried. The general fix comes with the next gap: the
   race's best sets go to Best play as separate uses, once a simulated round values the deck
   it leaves.
2. **The simulated round only reports planets, money, hands left and the best card it drew
   for a held consumable (`seen`).** It should report the board, deck and consumables it ends
   with, valued by `LongRun`, and let the look-ahead weigh more than seal planets.
3. **Valuation is still split.** Deck changes are valued in `value.rs` (`deck_stats`) but not
   yet through `Gain`, and the money a deck's cards earn becomes income past a $0.5 threshold,
   decided once (in the target race, from its first batch: a fixed offset its paired test
   can't see); the voucher, tag and Emperor
   formulas, and the shop ranking spend leftover money on rerolls in each of its three
   factors. The general fix: deck, joker and hand-size fields in `Gain`, and each dollar spent
   once in one model.
4. **The simulated player's policy (`decide_cards`) is a rulebook,** partly keyed to jokers, and
   only ever chases flushes. The general fix: compare policy moves with a cheap search using
   the engine, as the look-ahead does at "win now".
5. **Shop ranking and skip-or-play aren't confidence-based.** They tie within 3% instead of
   using `compare.rs`, because their options are valued by separate simulations, not round by
   round on the same draws.
6. **Simulated discards change nothing but the hand.** Jokers that change with discards used
   (Green Joker, Ramen, Castle, Yorick, Hit the Road, Burnt Joker, Trading Card) and money paid
   for discards left (Delayed Gratification) aren't modelled there (discard money from Mail-In
   Rebate and Faceless Joker is: `engine::discard_money`): e.g. with
   Mystic Summit and Green Joker, burning discards looks free. The general fix belongs in the
   engine's discard step (the game's own discard effects), not in the policy.
7. **The long-run projection spends money on your main hand's planets.** A board that wants
   several hands levelled is valued as if it wanted one.
8. **`advise.rs` still holds the shop options, tarot valuation and outlook inline,** and the
   outlook's heuristics note says growth isn't projected, which is out of date (`grow_antes`
   projects it).
