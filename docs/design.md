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
| **Moves tried** | Which choices are compared? All of them, narrowed down by simulation, never a hand-picked few. | Best play (`advise/play.rs`): `sim::all_plays` (every 1–5 card play) and `sim::all_discards` (every discard), for your hand as it is and after each held consumable, all in one `compare::race`. Shop: `advise.rs` `shop_options` (every shop item, pack pick and "leave"). Skip-or-play. A consumable's targets: every set of cards its game data allows (`engine::consumable`; a random effect on them, Aura's edition, as its outcomes with their chances, a set worth its outcomes' values on each round), in the hand it's used from, in one `compare::race` on the deck projection (`target_race`). A joker that changes a card each round (DNA: `consumable::round_card_effect`) goes through the same search, round by round (`LongRun::round_decks`). |
| **Simulated player** | How is the rest of the round played out? Toward the same measure the advice uses. | `sim::decide` (play/discard policy, a labelled heuristic: dig plans, `sim::aims` (a flush, a straight, one more of a rank, a Full House from Two Pair), each by its draw odds × what completing it scores; pace by a play's average score, `Play::mean`; cards that score while held kept, `held_value`), measured by `tests/player.rs` (win rates on one board per play style at fixed targets, and against `sim::set_oracle`, a slow player that tries alternatives on simulated futures), `sim::RoundGoals` + `play_on_instead` (weighs "win now" against playing on, with a few simulated futures), `sim::pays_at_end` (cards that pay at round end stay in hand while on pace), held consumables: used for score as soon as they lift the best play, the biggest lift first (`sim::use_if_better`), except one that would put a card that pays at round end in hand, which is held while on pace; right before the winning hand, each that makes the win worth more by `RoundGoals::value` is used (`sim::finish_with_uses`: a Blue Seal or Gold card more in hand at the end, a slot freed for a planet). The "play on" futures play to the same goals, the finish included, without looking ahead again |
| **Valuation** | What is an outcome worth? One measure, measured by simulating your board. | `advise/value.rs`: `LongRun` (your board projected to Ante 8; `value(&Gain)` for anything a run gains: one-off money, money held, planets with Constellation, levels, consumable slots (the planets your Blue Seal cards make with them: `seal_planets_for`; your slots by Ante 8 are a projected quantity, `consumable_slots`, as hands and hand size are), hand size, hands and discards a round (`Gain::hand_size` / `hands` / `discards`: added to the projected round, `long_spec_for`, so it's played with them, and to the round your Blue Seal cards are seen in: every projected board is scored on its own round, its jokers' and a `Gain`'s, with the planets your Blue Seals make more or fewer in it than in yours, `long_score`, `seal_planets_for`), the events between rounds the board grows from (pack skips, rerolls, blind skips: `engine` `Board::after`, the game's amounts via `Joker::mult_from` / `xmult_from` and its counters; money, once or held, buys them through one chooser, `bought_event`, at `event_price`); `planet(hand)`; `long_of` for jokers), `Spending` (money held past the planet-level cap, worth the rerolls and packs it buys). `RoundGoals` for the simulated player is built from these. Shop ranking: `rank_options` (survive × next-ante survival × long-run). |
| **Noise** | Which differences are real? | `advise/compare.rs` `race`: the same draws for every option, in batches on fresh rounds until the best is clear; options clearly worse drop, options shown within 1% of the leader's value are ties, reported as ties (showing it takes enough shared rounds for a rare round where they part ways to have turned up: 3 × the largest round value / n ≤ 1% of the leader's value; a tie stands only through ties to the final leader; tied moves are ordered by their paired difference to the leader); a race with a status quo (a target search's "no target") never cuts it by the budget, only by a finding, and keeps it when it ends as good as the leader and not acting keeps something (D8: a held consumable isn't spent on a change that can't be told from none; DNA's round, gone if unused, takes the leader); options still in at the round cap are "undecided" and every caller reports them as ties too (as good as far as those rounds can tell; at a low cap, such as the target search's 128 rounds, that's all a tie can be, since showing one needs about 300); past that, an explicit budget (by value, then a secondary value such as how close lost rounds came to the target) limits how many stay in, and is named as a budget, not a finding. The search is measured against a reference (every move on `compare::MAX` rounds of its own: `search_against_reference` in tests/replay.rs). Same seeds (common random numbers) everywhere; cards are put in one fixed order (`Card::order_key`) before anything random touches them, so card order never changes the advice (tested). |

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
| Spare money (held above the interest line, each ante) and one-off money are one budget: on a board with a joker that grows from shop events, it's split levels first (up to what `levels_per_ante` can use) or events first (up to what the shops hold), the rest on the other, whichever the projection scores higher by more than 1% (else levels first: `spare_to_events`, decided once per set of such jokers, on your projected board with that option, slots filled), each dollar once; one-off money takes the same route (a price takes back events, then levels; Constellation follows the planets bought); levels $20 an ante held (`levels_per_ante`) or $12 once, added to the level already there (`add_levels`, a fraction included); $4 any planet (the same planets both level and grow Constellation) | `value.rs` `fill_long`, `spare_to_events`, `spend_once`, `add_levels`, `event_price` | what spare money buys | logged shops |
| Rent $9 an ante | `value.rs` `RENT_PER_ANTE` | $3 a round × 3 | exact |
| 48 rounds per projected board | `value.rs` `long_score` | projection noise | compare against more rounds |
| Growth by Ante 8: a reroll an ante done anyway; a pack skipped only when the budget buys it (skipping gives up what the pack holds, so it costs a pack: $4), at most the 6 packs an ante's shops hold (2 a shop, game.lua); rerolls at the game's price (+$1 for each before it in a shop; one-off money buys them at the first price), at most 5 bought an ante; packs from skip tags are counted in skip-or-play; every joker that grows from an event grows; 12 hands an ante, per-joker rates for in-round growth. The dig list's growth label (and next-ante reach) counts only the reroll done anyway; what spare money buys is By Ante 8's | `value.rs` `fill_long`, `events_per_ante`, `PACKS_PER_ANTE`; `advise.rs` `grow_antes`, `grow_one_ante` | how growing jokers grow | logged runs |
| Madness horizon 1 ante | `value.rs` `long_of` | it eats your jokers | — |
| Spending: level cap reached at the interest line + $30; packs $4 (`PACK_PRICE`), 2 a shop, each further one ×0.8; typical pack by shop weights (game.lua 4/4/1.2/0.6/4), Standard counted as 0 | `value.rs` `Spending` | money past the cap buys rerolls and packs | logged packs opened and picks |
| Rerolls: up to 8 a shop; each further joker bought counts ×0.6 | `advise.rs` `money_value_with`, `expected_buys` | joker gains don't simply add up | logged rerolls |
| A dollar in Best play: what +$10 does, taken as linear | `play.rs` `dollar_gain` | money won this round | — |
| Comparison: at most 1,600 rounds an option, "clearly worse" at 95% (paired), "equal" within 1% of the leader's value | `compare.rs` `MAX`, `EQUAL` | when a difference is real enough to act on | — |
| Best play's search budget: first batch 32 rounds (batches double); at most 48 undecided moves after 32 rounds, 24 after 64, 12 after 128, 8 after that, kept by value then how close lost rounds came to the target (a first cut at 16 rounds ranked moves on money before a rare lost round could show: the safest discards fell out) | `advise.rs` `SEARCH_FIRST`, `SEARCH_BUDGET` | cost vs. coverage | `search_against_reference` (2026-10-03, seeds 42-44, the 6 fixtures in a blind; a pick passes when gap − 2·se ≤ 1%): see known gap "search misses" |
| A tie (two options within 1%) takes n shared rounds with 3 × the largest round value / n ≤ 1% of the leader's value (about 300 when a round is worth about the leader's value): a round where they part ways may be rare, and when none has turned up in n rounds it may still happen 3 in n (95%, the rule of three) | `compare.rs` `paired` | the rule of three | — |
| Which hands carry your points (the main hand planets go to, joker growth, the style outlook): the hardest round of the ante, played from the start (a round in progress begins again, with the blind's own rules) | `advise.rs` `hand_mix`, `fresh_round`, `blind_from_start` | your hands in a typical round of that blind | — |
| Simulated player's policy: a dig plan (`aims`: a flush of a suit you hold 2+ of; a straight you hold all but 1 or 2 ranks of, with Four Fingers and Shortcut; one more of a rank you hold 2+ of; a Full House from Two Pair) chased when its odds are > 10% and odds × its score beat the best hand's: × (hands − 1, 1 to 2) against the best hand × hands when behind, 1.5× when on pace (then only one card away and when playing the best hand would spend 2+ of its cards); odds exact for a flush (`flush_odds`), else the cards the digs would see as one draw (`draw_odds`: exact for one card away when every other card is thrown each dig; cards kept for their held value aren't taken off the free slots, and a straight two ranks away counts only its likeliest pair of ranks); its score the average over its completions (`aim_completions`: every distinct card that fits, by its copies, when one is missing from each group, else 4 seeded random draws; a fixed typical card was biased both ways: neighbouring middle ranks made flush draws straight flushes, spread ones never did), screened on its middle completion, the best 2 in full (`AIM_FINALISTS`); duplicates (Smeared Joker) once; a straight keeps the copy of a rank in the suit you hold most; a straight two ranks away only when no straight one rank away keeps the same cards; the play itself chosen by its floor (every roll failing), pace and the chase on its average over 8 fixed rolls (`MEAN_ROLLS`); cards that add to the best play while held never thrown away, nor played in a dig; discards that pay money (`engine::discard_money`) cashed while on pace (a threshold, not yet weighed against `RoundGoals` money), the best-paying one whose remaining hand keeps pace, at most 16 tried, the last one right before the round ends; discards burned first when the chosen play scores > 0.1% more with none left (`burn_pays`; which cards, the policy's own dig choice) | `sim.rs` `decide_cards`, `aims`, `aim_odds`, `aim_score` | how a player digs | `tests/player.rs` (2026-10-04: 50.1% → 62.5% at the fixed targets; against the oracle; D9); replay fixtures; calibration of win chances |
| Cards worth drawing this round: for each held consumable that goes on one card (one that takes more isn't counted: a race per card kind costs too much, so a pack card picked for an Empress isn't "worth drawing" later), its value on each kind of card in the draw pile minus its value on the target its search chose in hand (48-round screen, kept when over 1% on the full projection; Blue Seal planets counted); tied to its consumable and gone once that's used; a simulated round counts the best one it draws | `play.rs` `seen`, `sim::RoundGoals::seen` | a better target may come | — |
| A consumable's targets (every one that goes on cards you pick): every target set in the hand it's used from (yours when it's held or in the open pack and a hand is on screen, else 4 sampled hands for one you can buy, pick or hold, 1 for the rest of the pool: noisy, and in a blind it reaches Best play's money value through pack values), plus no target (your deck as it is), sets leaving the same decks counted once, a random effect's outcomes (Aura) each valued on the same rounds and weighted by their chances, chosen by `compare::race` on the deck projection (first batch 16 rounds; at most 12 sets undecided after 16, 6 after 32, 3 after that; at most 128 rounds, however many sets a big hand gives; the money a set's cards earn becomes income once, from its first batch, against your deck's money on the same rounds), on rounds of their own (after the 300 the pick is then valued on); no target never cut by the budget and, deciding a use on the hand on screen (the consumable stays held), kept when it's as good as the best (D8; valued on sampled hands, the best set is taken); the sets it can't tell apart from the best are listed (at 128 rounds the race can't show a tie, so these are the sets still in at its cap); the rest ordered by their paired estimate against the best; a deck's extra Blue Seal cards counted as planets (`seal_planets_in`, 3 rounds an ante) | `advise.rs` `TARGET_FIRST`, `TARGET_MAX`, `TARGET_BUDGET`, `value.rs` `deck_rounds`, `deck_value` | cost vs. accuracy (a first batch of 8 dropped the best targets) | check if better targets are ever cut by the budget |
| Random effects (Immolate, Familiar, Grim, Incantation, Aura, Sigil) and the shop's sampled hands: valued over several outcomes (6 for the destroyers; Aura's 3 editions at the game's chances, on the target its search chose), the projection's 300 rounds split between them (however many outcomes: the target search picks on the rounds after them), each outcome on rounds of its own (`deck_value_part`, through `decks_value`); this round's odds for the destroyers come from one outcome | `advise.rs` `RANDOM_OUTCOMES`, `decks_value` | the average outcome | — |
| DNA (a joker that changes a card each round): one sampled opening hand a round (3 rounds an ante; discarding first to dig isn't tried; when it isn't offered now, only in the dig list's pool, one race an ante, its pick made in each of the ante's rounds: a third of the cost, a coarser deck), drawn from the deck with the copies so far, each round's race on rounds of its own, its best set copied as a consumable's is (a set that leads on noise included); what comes round by round is a flow, in the race (`UsePrice`) and the value alike: the hand a copy spends (that round's share of a hand fewer every round, measured once on your projected board, the Blue Seals it sees fewer included: `hand_cost`) and the planets its Blue Seal cards make (from the round it's made); the deck valued as the one it ends with by Ante 8 (as a grower's state) | `value.rs` `round_decks`, `round_effect_long`, `hand_cost`; `advise.rs` `UsePrice` | the cards you'd copy, and when | logged runs with DNA |
| Typical hands with jokers that pay by discards left: scored with your discards and with none, the better counts (only when a reference Pair scores differently) | `advise.rs` `Ctx::typical_n` | you'd keep or burn them | — |
| Look-ahead: 8 futures, only when a better finish could add ≥ 1% in seal planets | `sim.rs` `play_on_instead`, `LOOKAHEAD_ROLLOUTS` | cost vs. depth | replay fixtures |
| Consumable used in a simulated round when it lifts the best play by > 1%; one that would put a card that pays at round end in hand (counted on the board after it's used: the slot any use frees doesn't count) is held while the best hand times hands left covers what's still needed (on pace); equal gains by key, not slot order | `sim.rs` `use_if_better`, `finish_with_uses` | when a player uses one | — |
| Blue Seal: drawn with the hand plus 3 cards an action, each card independently (in the projection the round of the board scored: its jokers' hand size, hands and discards, `long_spec_for`, plus a `Gain`'s, against your projected board's, `LongRun::round`; this round's for skip-or-play); a round's planets the expected number drawn that fit your consumable slots (E[min(drawn, slots)], card.lua `get_end_of_round_effect`): in the projection your slots by Ante 8 (`LongRun::consumable_slots`: today's, less the one each Negative consumable you hold adds, which goes with the card; plus a voucher's, `Gain::consumable_slots`) less the consumables held at round end (below), the ones free now for this round's end (skip-or-play: the money cards it counts as used don't hold one); each deck's own Blue Seals at its own size; Mail-In rank 4 in 13 | `advise.rs` `seal_round_chance`, `seal_planets_a_round`, Mail-In income; `value.rs` `seal_planets_in` | how often a card shows up | game source + logged rounds |
| Consumables held at round end, besides the planets Blue Seals make there: 1 in 4 rounds (none or one, mixed in that share), the same count whatever your slots | `value.rs` `HELD_AT_ROUND_END` | slots the planets can't have (card.lua `get_end_of_round_effect`: a Blue Seal's planet needs the consumables held to be fewer than the slots) | **measured** in the calibration log (2026-10-01): the consumables in each round's first shop, less planets new since the last shop, on the rounds that had Blue Seal cards (one run, 99XSP7C7, rounds 10–13, 2 slots: one held in 1 round, none in 3; over all 13 rounds 7, mostly a Cryptid held before there were Blue Seals). Few rounds, and the logged player followed advice that never charged for holding: log the deck's Blue Seals and the slots with each shop to remeasure, per slot count |
| Glass cards: about 1.5 scores an ante before breaking; what a deck's extra Glass cards add (against the deck with them destroyed, as breaking does) counts for the share of the antes left they last, the rest of the change in full | `advise.rs` `glass_presence`, `value.rs` `without_new_glass` | Glass breaking | — |
| A Standard pack card with a consumable you hold on it: for every held one that goes on cards (`engine::consumable`), one race (`target_race`) over a hand you'd hold with the card in it (the first hand its own value is sampled on, one card swapped for it); credited only when the best targets include the card, nothing the race can't tell apart from them leaves it out, and the best set with the card is clearly better (paired, 95%) than the best set without it (by paired estimate), as the card × (with it used on the card) / (with it used on the best set without it), paired on the same rounds; labelled "assuming you draw it while you still hold" it | `advise.rs` standard pack cards, `target_race` | you'll draw it and keep the consumable for it | the chance to draw it before you'd use the consumable elsewhere (`seal_round_chance`) |
| Deck changes (tarots, pack cards): 300 rounds against your deck (split between outcomes for random effects); a board without a chips joker (`CHIP_JOKERS` list) gets a +60 Chips find | `value.rs` `deck_stats`, `TAROT_ROUNDS` | deck changes are small; a chips gap closes by Ante 8 | — |
| Vouchers: Overstock half a reroll a shop, Reroll Surplus $2 a shop, Clearance 25% of ~$8, Hone one buy an ante, a voucher shows 1 in 16, Planet Merchant 1 in 12 on your main hand; one that changes your consumable slots (Crystal Ball) or what a round starts with (Grabber, Wasteful, Paint Brush: hands, discards, hand size) by what it changes in the run (`plan::apply_voucher`, card.lua `Card:apply_to_run`), through `Gain::consumable_slots` / `hand_size` / `hands` / `discards` with its price (`voucher_gain`; a slot's only by its Blue Seal planets: room to hold more tarots and planets isn't modelled, and says so; a hand's money left at cash-out isn't, and says so); this round's odds with it from the same change, the blind's rules on top (`blind_from_start`: The Needle, The Water) | `advise.rs` economy and voucher values | what each voucher saves | logged shops |
| Tags: Meteor's main-hand planet 5 in 12, best 1 of the pick 2; Investment not discounted for arriving later; a Mega Buffoon pack's second pick $2.50 (shop and tag); Emperor average². Any pack can be skipped once while a pick is left: a one-pick pack is worth at least the skip, a Mega pack's second pick at least the skip (`pack_value`); a skipped blind's Throwback growth counted by Ante 8, not for this ante's boss | `advise.rs` skip-or-play, packs, tarots, `pack_value` | — | logged tags and packs |
| Skip-or-play "close": within 3% | `advise.rs` skip-or-play | noise | (should become `compare.rs`) |
| Shop ranking: ties within 3%; under 20% this round, score reach decides; next-ante survival dropped when under 1% for every option; a sell choice may cost at most 5 points this ante | `advise.rs` `rank_options`, `NOW_SLACK` | noise and hopeless rounds | (should become `compare.rs`) |
| Perishable gone by Ante 8 if its rounds < 3 × antes left; Gros Michel breaks 1 in 6 a round | `value.rs` `lasts`, `advise.rs` | — | game source (exact) |

## Retire list

Narrow rules still in the code, each to be replaced by its stage's general mechanism. Don't
add to this list's kind; remove from it.

| Rule | Where | General replacement |
|---|---|---|
| Suit-joker map, `kickers_matter` list, `income_per_ante` per joker | `sim.rs` `keep_suit`, `advise.rs` | game facts: move to `data/` or the engine; or work out what to keep by scoring with and without a card |
| Constellation grows from planets in several places | `value.rs` `add_planets`, `spend_once`; `advise.rs` Meteor tag, `grow_constellation` | a planet used as an event in `engine::RunEvent`, applied by `Board::after` (`xmult_from`), counted through `Gain::planets`; `bought_event` then compares what money buys by measured value, not Mult per dollar |
| A short perishable's price dropped from its value | `advise.rs` (`long_mult = 1 + unlock`) | `LongRun::value` with its price |
| What a joker adds to a round (hand size, hands, discards) as a table, and Turtle Bean's shrink by name; Burglar counted as if it changed the round you start with (it acts when the blind is set: from memory of card.lua, not checked) | `advise.rs` `round_mods`, `value.rs` `long_spec_for` | the center's config in `data/` (`h_size`, `d_size`, `extra`) as game facts; a fading joker's state projected as growth is (`grow_antes`); "set to N" effects (Burglar's discards, The Needle, The Water) applied after the additive ones |

## Known gaps

Ranked by how likely each is to cause the next round of patch-on-patch.

1. **Best play's search still misses sometimes.** Measured by `search_against_reference`: the
   budget cuts moves on few rounds (losing is rare, so early rounds rank moves on money). With
   the dig plans in the simulated player (2026-10-04) every pick passes (wall_cryptid seed 44,
   1.64% ± 0.51% below the best before, the Cryptid-first Flush cut at 64 rounds, now picks a line
   1.5% ± 0.6% above the reference's best), but nothing in the budget changed, so the risk stays.
   The general fix is a cut that can't drop a move a rare lost round would separate; check
   each change against the reference on several seeds.
2. **Best play and the simulated round try one target per consumable** (a simulated round
   that draws a better target doesn't switch to it), the one its search chose by the Ante 8
   projection (ties listed, not weighed by this round), as its "use now" move. Targets that win this
   blind but leave a weaker deck aren't tried. The general fix comes with the next gap: the
   race's best sets go to Best play as separate uses, once a simulated round values the deck
   it leaves.
3. **Random effects and DNA reach only part of the search.** Aura's random edition isn't a
   Best play move (random outcomes aren't turned into `sim::Use`) nor counted in "cards worth
   drawing" (`play.rs` reads `consumable::card_effect`, certain effects only); DNA's race
   prices the hand a copy costs by a share measured apart (on your projected board, not round
   by round on the same draws), and tries only the opening hand, not the cards you'd see after discarding first (card.lua: DNA checks
   `hands_played == 0`). Blue Seal planets are counted by `sim::seal_planets` (cards actually
   held, free slots) in a simulated round and by `seal_planets_a_round` (expected, the slots your other consumables leave at round end)
   in the projection. The general fix: random outcomes as moves with weights in Best play, and
   a hands field in the deck projection so DNA's race weighs the hand on the same rounds.
4. **The simulated round only reports planets, money, hands left and the best card it drew
   for a held consumable (`seen`).** It should report the board, deck and consumables it ends
   with, valued by `LongRun`, and let the look-ahead weigh more than seal planets.
5. **Valuation is still split.** Deck changes are valued in `value.rs` (`deck_stats`) but not
   yet through `Gain`, and the money a deck's cards earn becomes income past a $0.5 threshold,
   decided once (in the target race, from its first batch: a fixed offset its paired test
   can't see); the voucher, tag and Emperor
   formulas, and the shop ranking spend leftover money on rerolls in each of its three
   factors (the projection's spare money is one budget for levels or events, but not yet for
   the rerolls and packs `Spending` and `money_value_with` count); a price lowers
   Constellation and main-hand levels below what you already have (only event Mult is
   protected); a concrete pack's skip counts as one more on top of the projection's; DNA you own isn't projected with its copies (only one you could buy is: `round_effect_long`), and one you'd buy is its board (one hand fewer) times its deck change, measured apart; a random effect's outcome parts (Aura valued on 4 sampled hands: 12 parts of 25 rounds) each decide their money-as-income on their own few rounds; the rerolls `Spending`
   and `money_value_with` assume don't grow Flash Card; the projection keeps a joker debuffed by
   the current boss debuffed (so it doesn't grow either). The general fix: deck, joker and hand-size fields in `Gain`, and each dollar
   spent once in one model.
6. **The simulated player's policy (`decide_cards`) is still a rulebook** in its thresholds
   (10% odds, 1.5×, hands − 1 capped at 2), though it now digs for four kinds of hand by their
   draw odds × the engine's score. A search inside every simulated round (the oracle in `tests/player.rs`)
   costs hundreds of times the policy, and the advice runs simulated rounds everywhere, so it
   only measures (its alternatives ignore Smeared, Wild, Four Fingers and Shortcut, so its gap
   is understated on those boards). Known misses: a plan's score counts only the cards that
   complete it, not what else the chase leaves in hand; with Four Fingers + Shortcut the
   player is about 5 points below the old one at its target with 3 discards, and the oracle
   wins 13.7 ± 3.0 points more there: not the kinds of plan (each alone gives the same), most
   likely the thresholds (the old player chased flushes more, on an inflated score, and
   straight flushes are common there), not yet shown; on pace, a chase is weighed by score (1.5×), not by what the
   round is worth (`RoundGoals`: spare hands, discard money), though the round is likely won
   either way; plans cover four kinds of hand (not Two Pair from a Pair, a Full House from
   Three of a Kind, a straight flush draw); face-down cards are planned with as if seen. The
   general fix: tune the thresholds against the oracle, decide an on-pace chase by
   `RoundGoals::value`, and score a plan on the hand it leaves.
7. **Shop ranking and skip-or-play aren't confidence-based.** They tie within 3% instead of
   using `compare.rs`, because their options are valued by separate simulations, not round by
   round on the same draws.
8. **Simulated discards change nothing but the hand.** Jokers that change with discards used
   (Green Joker, Ramen, Castle, Yorick, Hit the Road, Burnt Joker, Trading Card) and money paid
   for discards left (Delayed Gratification) aren't modelled there (discard money from Mail-In
   Rebate and Faceless Joker is: `engine::discard_money`): e.g. with
   Mystic Summit and Green Joker, burning discards looks free. The general fix belongs in the
   engine's discard step (the game's own discard effects), not in the policy.
9. **The long-run projection spends money on your main hand's planets.** A board that wants
   several hands levelled is valued as if it wanted one.
10. **Packs are valued in two places, sized by their label.** A shop pack and the same pack
   from a skip tag have their own branches (Meteor's Mega Celestial by explicit planets, the
   shop's by an average), and "Mega" / "Jumbo" are read from the name, not the center's
   `extra` / `choose`. In an open Mega pack, picking one card and then skipping the rest (for
   Red Card) isn't an option: the picks left aren't read from the save. The general fix: one
   pack valuation from the center config (`pack_value`'s rule), used by shop packs, tags and
   the open pack.
11. **`advise.rs` still holds the shop options, tarot valuation and outlook inline,** and the
   outlook's heuristics note says growth isn't projected, which is out of date (`grow_antes`
   projects it).
12. **Consumable slots, hands, discards and hand size grow only by a voucher bought now, and a
   slot only counts for Blue Seals.** Your slots and your round (hand size, hands, discards) by
   Ante 8 are projected quantities (`LongRun::consumable_slots`; `long_spec_for` from the
   jokers of the board scored), and a voucher that adds to them (Crystal Ball, Grabber, Wasteful,
   Paint Brush) is valued through the `Gain` field for it (`voucher_gain`): the projected round
   is played with it and your Blue Seal cards are seen in it, as they are in the round of a
   joker that changes it (Juggler, Stuntman: `long_score`; not yet in the choice of which of
   your jokers a typical find replaces, made before your board is known). But a voucher you
   might buy later isn't projected (none is); what a slot more does besides Blue Seals (holding a
   tarot for its target, planets for Observatory, Perkeo) isn't modelled; holding a consumable
   costs no Blue Seal planets (the consumables held at round end are a measured constant, not
   what you hold or would buy: holding one for k rounds costs about `seal_planets_for(b, (0, 0, 0), -1)` ×
   k / rounds left); nor does keeping Blue Seal cards in hand for them, since projected rounds
   play to an unbounded target, never "on pace", so the simulated player's keep
   (`sim::pays_at_end`, against the board's `planet_slots`) never acts in the projection (deck
   changes, DNA and Crystal Ball alike). A round's extras reach the round and the Blue Seals,
   not the rest that reads today's round: joker growth in rounds counts 12 hands an ante
   (`grow_antes`), Mail-In's income today's discards (`income_per_ante`), DNA's opening hand
   today's hand size (`round_decks`). Projected rounds are played out with no target, so a hand
   more leaves none to pay money at cash-out (`money_per_hand`; said in the note), and a discard
   more is worth what the simulated player makes of it when it's always behind (its dig
   threshold scales with the hands left against the best hand, not with what's needed): about
   nothing on the boards measured (×0.99–1.03 on 400 rounds), so Wasteful comes out near its
   price. Next-ante survival doesn't see a voucher (nor a planet or tarot: only a joker gets its
   own next-ante odds). Antimatter (a joker slot) isn't valued by Ante 8: a
   slot is worth the joker you'd fill it with, which needs the shops ahead (the projection's
   stand-ins fill the slots you have). The general fix: held at round end as an output of the
   simulated round, projected rounds played to the target of the blind they stand for, and the
   projected round read wherever the projection reads a round. The economy, Hone / Glow Up and
   Planet Merchant vouchers are still valued by their own blocks (Planet Merchant with its own
   planet rate beside `apply_voucher`'s).
