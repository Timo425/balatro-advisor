# balatro-advisor: design

How the advice is built, and how to change it. Read this before changing what the advice
says. The required workflow for a change (fixture, diagnosis, fix, proof, review, record) is
in `CLAUDE.md`.

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
| **Moves tried** | Which choices are compared? All of them while the search can take them, narrowed down by simulation; beyond that, the moves the hand's structure gives, over every card (D10): no cut by the cards' order, the budgets named in the register. | Best play (`advise/play.rs`): `sim::all_plays` (every 1–5 card play) and `sim::all_discards` (every discard) of a hand of up to 12 cards (`sim::ALL_MOVES_MAX`); a bigger hand's `sim::big_hand_moves` (the plays whose cards can all score, `scoring_plays`, each also topped up with the least-worth cards outside it, the best few of each kind then by score now, 256 in all, a budget; the discards of the hand's structure, `structure_discards`), for your hand as it is and after each held consumable, all in one `compare::race`. Shop: `advise.rs` `shop_options` (every shop item, pack pick and "leave"; in the shop, selling each sellable joker outright too: its rounds without it from the contribution pass, its sale, the run with it let go, `LongRun::sold_now`, ranked like every option); a joker with no slot free, in place of each joker whose sale makes room (not eternal, not Negative: its slot goes with it) and, in the full pass, in each place it can go (the freed slot and the right end; a sale's places that play the race's first batch the same are one) (`evaluate_candidate`; only a perished one, if any), all in one `compare::race` on the shop ranking's measure (`shop_measure`, one function for both: through this ante's hardest round × the next ante's boss × the geometric mean of the long run with that sale, `LongRun::long_of`, and the strength at the next ante; in the race per round, `SwapRound::value`, with Mr. Bones' save counted), those it can't tell apart from the pick reported as ties (`Candidate::sell_tied`; none in trouble, as the shop ranking): what a joker does outside a hand (Burglar's hands, Drunkard's discard, Mr. Bones' save, Card Sharp's later hands) shows in the rounds, what it leaves the run (its income, its growth) in the long run; with a free slot, the two places of a joker on offer raced the same way; the pool's screen tries only the joker whose removal leaves most by the same measure in expectation (`Ctx::removal_rank`, `swap_measure`, from the contribution pass; the outlook gives up the same). Skip-or-play: the blind you're choosing, or in the shop (no pack open) the next blind still to come, with the money you have now. A consumable's targets: every set of cards its game data allows (`engine::consumable`; a random effect on them, Aura's edition, as its outcomes with their chances, a set worth its outcomes' values on each round), in the hand it's used from, in one `compare::race` on the deck projection (`target_race`). A joker that changes a card each round (DNA: `consumable::round_card_effect`) goes through the same search, round by round (`LongRun::round_decks`). |
| **Simulated player** | How is the rest of the round played out? Toward the same measure the advice uses. | `sim::decide` (play/discard policy, a labelled heuristic: dig plans, `sim::aims` (a flush, a straight, a straight flush, one more of a rank, a Full House from Two Pair, Two Pair from a Pair, a Full House from Three of a Kind), each chased dig by dig as the policy would, when the round's expected points with it beat playing on (`sim::chase`; after a first dig that discards, the plan's completion, the lesser plans and the hand a failed chase is left with scored on the board its cards leave, and a later hand played on by what that board makes of the best play now: `dig_board`, what the engine's step, `Board::discard`, does to the board's state, the count of discards left put back); where which card is thrown changes what a score reads, a discard throws first the cards whose discard alone leaves the best play of the cards that stay scoring most, then the lowest (`throw_order`: Hit the Road's Jacks, Castle's suit; an effect every card has alike, Green Joker's, changes nothing); pace by playing on (`playing_on`: the best play's average now, then the best play of the same cards each hand after on the board the hand before leaves (among the plays a full search left unbeaten in chips and Mult, `later_best`: the decision's own search of the hand, searched again when a hand's step moves plays of one hand apart; every play when The Hook's pick matters, `front_holds`), the engine's own step, `Board::after_hand`, but not The Eye's ban on the hands played since: the cards held stand in for the ones drawn); cards that score while held kept, `held_value`; the round plays on the board each play and discard leaves: the engine's steps, `Board::after_hand` (jokers' state, the hand's level, money, Ice Cream and Seltzer after the hand) and `Board::discard` (card.lua's discard contexts; The Hook's through `discard_by`), and a won round's money for how it was played: Gold cards held and every joker's `calculate_dollar_bonus`, `engine::won_round_money`; the hands left counted apart at `money_per_hand`), measured by `tests/player.rs` (win rates on one board per play style at fixed targets, plus boards with jokers that grow in the round, a deck with a few strong cards and debuffing bosses; and against `sim::set_oracle`, a slow player that tries alternatives on simulated futures; `player_values`: the round's value on boards with fixed goals), `sim::RoundGoals` + `finish_play` (the winning play worth most) + `play_on_instead` (weighs "win now" against playing on, with a few simulated futures, paired: `paired_pick`; playing on includes the play that grows the jokers most, `growth_play`) + `spare_instead` (with no win on the table, the policy's play gives way to the highest-floor play that grows the jokers clearly more, `sparing_play`, when the round played on after it keeps pace and scores as much as after the policy's play and 16 paired futures show it worth more by `RoundGoals::value`), `sim::pays_at_end` (cards that pay at round end stay in hand while on pace), held consumables: used for score as soon as they lift the best play, the biggest lift first (`sim::use_if_better`), except one that would put a card that pays at round end in hand, which is held while on pace; right before the winning hand, each that makes the win worth more by `RoundGoals::value` is used (`sim::finish_with_uses`: a Blue Seal or Gold card more in hand at the end, a slot freed for a planet). The "play on" futures play to the same goals, the finish included, without looking ahead again, and by the policy alone when the oracle is on (`sim::policy_outcomes`, as the oracle's own futures) |
| **Valuation** | What is an outcome worth? One measure, measured by simulating your board. | `advise/value.rs`: `LongRun` (your board projected to Ante 8, each joker you could sell kept or let go, whichever leaves the run more by its value: replaced by a typical find in its best slot, or sold outright, no find, its slot left empty to Ante 8, `Fate`, its sale counted once either way, a find's price on top for a find; a Negative only sold, and its slot goes with it whenever it leaves the board, card.lua `Card:remove_from_deck`, as one bought brings its own; `value(&Gain)` for anything a run gains: one-off money, money held, planets with Constellation, levels, consumable slots (the planets your Blue Seal cards make with them: `seal_planets_for`; your slots by Ante 8 are a projected quantity, `consumable_slots`, as hands and hand size are), hand size, hands and discards a round (`Gain::hand_size` / `hands` / `discards`: added to the projected round, `long_spec_for`, so it's played with them, and to the round your Blue Seal cards are seen in: every projected board is scored on its own round, its jokers' and a `Gain`'s, with the planets your Blue Seals make more or fewer in it than in yours, `long_score`, `seal_planets_for`), the events between rounds the board grows from (pack skips, rerolls, blind skips: `engine` `Board::after`, the game's amounts via `Joker::mult_from` / `xmult_from` and its counters; money, once or held, buys them through one chooser, `bought_event`, at `event_price`); `planet(hand)`; `long_of` for jokers; `joker_worth`, what one more Mult, ×Mult or Chip on a joker is worth and losing it, through `Gain::joker`), `Spending` (money held past the planet-level cap, worth the rerolls and packs it buys). `RoundGoals` for the simulated player is built from these (a Blue Seal's planet by hand, a dollar, cards worth drawing, and what the jokers a round changes are left with: `RoundGoals::jokers`, counted at each hand and discard as `Outcome::grown`; in Best play's round each also by what it does in this ante's blinds still ahead, `Ahead`, and there also the counter a joker's ability keeps that a round moves, `Joker::counter`). Shop ranking: `rank_options` (survive × next-ante survival × long-run); a joker that saves a lost round (Mr. Bones, `Board::saves_lost_round`, spent once for each copy) counted in both survivals through the rounds ahead, on the board each option leaves (`save_lift`). |
| **Noise** | Which differences are real? | `advise/compare.rs` `race`: the same draws for every option, in batches on fresh rounds until the best is clear; options clearly worse drop (where a round can be lost outright, a steady small deficit isn't clearly worse until a rare round in its favour could have shown: the rule of three), options shown within 1% of the leader's value are ties, reported as ties (showing it takes enough shared rounds for a rare round where they part ways to have turned up: 3 × the largest round value / n ≤ 1% of the leader's value, and a round worth something: on rounds all worth nothing, a round you rarely win, nothing is shown equal, since 2026-10-11; a tie stands only through ties to the final leader; tied moves are ordered by their paired difference to the leader); a race with a status quo (a target search's "no target") never cuts it by the budget, only by a finding, and keeps it when it ends as good as the leader and not acting keeps something (D8: a held consumable isn't spent on a change that can't be told from none; DNA's round, gone if unused, takes the leader); options still in at the round cap are "undecided" and every caller reports them as ties too (as good as far as those rounds can tell; at a low cap, such as the target search's 128 rounds, that's all a tie can be, since showing one needs about 300); past that, an explicit budget limits how many stay in (with many options at least half of them for each doubling of the rounds, `compare::halving`: successive halving, so a field of hundreds isn't cut to a few dozen on its first batch), keeping those least shown worse than the leader (`compare::better_by`, the bound "clearly worse" uses: a move a rare round could still separate isn't cut for a steady edge on few rounds; an option whose rounds differ more from the leader's stays before a steady one as far behind, so in a graded race (the target search) what is still in at the cap leans to the noisier sets; since 2026-10-05, before by value), on exact ties by value, then a secondary value such as how close lost rounds came to the target, and is named as a budget, not a finding. The search is measured against a reference (every move on `compare::MAX` rounds of its own: `search_against_reference` in tests/replay.rs); a pick a change moves on the owner's saved states is judged against the one before with the rest of the round played by the oracle (`Options::judge`, tests/history_picks.rs and `history_judge` in tests/history.rs, D12). Same seeds (common random numbers) everywhere; cards are put in one fixed order (`Card::order_key`) before anything random touches them, so card order never changes the advice (tested). |

Layers underneath: `save` (reads the game's files), `engine` (the scoring pass, checked
against real in-game scores in the golden tests; the cards played and held are scored in the
order a player would arrange them, `engine::score`, since the game scores them in the order
they sit and the player drags them: each card's effect, worked out in the order given, is a
map of Mult, `mult → x·mult + b`, and sorting by b / (x − 1) leaves the most. Where no effect
read where a card sits and no card asked for a roll, the maps are exact and the order is
applied in the same pass; otherwise (an effect says when it reads a place: Photograph with two
face cards scoring, Hanging Chad, Raised Fist on a tied lowest rank) the order (and each half
of it alone) is scored as played, with every roll failing as a player arranges before any
roll, and kept when the whole hand scores more with no less money and the jokers left the
same; the order kept is then scored with the real rolls. So a score is always what the order shown
scores in the game, effects that read where a card sits included (Photograph, Hanging Chad,
Raised Fist); `score_as_played` keeps the order given, for the golden checks; Best play shows
the hand's order when the hand as it sits scores less), `sim` (rounds), `advise` (the four
stages above, and the output).

The engine is checked against every hand the owner plays (`handlog`): the save before a hand
and the save after it give the cards played (followed by their ids from the hand to the
discard pile, in play order; The Hook's reach the pile first), the cards held and the score
the game gave (the round's chips moved); the engine scores the same cards from the board
before. The live page logs each hand to `hands.jsonl` from a thread of its own, off the
analysis' path; `balatro-advisor hands` lists the mismatches (`--history`: the saved history
paired the same way) and `--capture` saves one to `tests/golden/` (`pending` while it shows a
known engine bug, D14's pattern). A hand with a roll (Lucky, Misprint, Bloodstone…) only has
to land between every roll failing and every roll hitting. On 2026-10-23 the history paired
863 of 1,434 hands: 742 exact, 120 in range, one off by a chip (Ramen's ×Mult drifts in floating
point, 2 − 0.01 × 18 = 1.8199999999999998, which the save writes as 1.82, so the game's floor
of 61151.99… isn't seen). Not paired: a hand that won the round (541: the save after it has
no cards left), a joker added or moved or a consumable used before the hand, a run that ended.

The owner's overrides are collected the same way (`choices`, `overrides`; `balatro-advisor
overrides`). The live page shows the advice before the owner acts, so a choice other than the
advice's is deliberate. `choices::step` reads the choice made between two saves in a row: a hand
(the hands log's pairing) or a discard (the cards' ids again), by the cards' places in the hand;
a buy (a joker bought in place of one sold is one choice, the shop ranking's swap), a sale, a
reroll, leaving the shop; a pick or a skip in a pack; playing or skipping a blind; the jokers'
order (only a move of the jokers whose order can change a score, `advise::order_free`, read
beside whatever else was done between the saves). The game
doesn't save after every click, so two choices between two saves are counted as unreadable,
never guessed, and so are a card taken from a Tarot, Spectral or Standard pack (what it changed
isn't followed), a consumable used and the round-winning hand (its cards are gone from the save
after it). Plays and discards are judged as the history check judges a moved pick: the owner's
move against Best play's quick pick with the oracle playing on (`Options::judge`, D12), the
verdict by the search's tie margin (yours ahead, the pick ahead, a tie, unclear). It is the
advice judging itself, and the output says so. Shop, pack and skip-or-play choices are graded by
agreement only: the ranking's top option, one shown tied with it, one ranked below or not among
its options; skip or play against `SkipCompare::verdict`; the jokers' order against the order
advice (`OrderAdvice`) on those same jokers. Nothing judges a shop choice yet, so no cost is
given. Overrides the judge can't show were worse are listed as candidate fixtures, each
with a capture command (CLAUDE.md step 1). The advice is today's code on the saved state, not
what the page showed then: the page keeps no analyses. It's for finding one questionable call in
a run (`--run SEED`) and capturing it, one at a time: a whole run's list goes stale as the advice
changes, so whole-session review, the page logging what it showed, and top players' runs are
parked (the owner's queue, 2026-10-25).

The parity tier (D17) checks the engine's steps against the game's own code, run headless.
`tools/parity/` is our own LuaJIT code, a stand-in for LÖVE and a job runner: the game's Lua is
read from the install in place (`unzip -p`, nothing extracted), the owner's settings and profile
as plain-text copies (unlocks shape the shop's pools). Each job loads a saved state through the
game's `Game:start_run`, takes one step through its button functions, runs its event queue until
it rests and writes the game's own save of what it leaves; the game rolls its own RNG from the
save, so a roll is the game's. `tests/parity.rs` (`--ignored`) makes the jobs from a sample of
the history chosen by name as the history check's is (8,000 states by default): the history's
own steps (hands, round-winning hands with their cards inferred, discards, blind setups,
cash-outs run on from the winning hand, shop rerolls, pack openings), checked against the
game's next save, which checks the harness; and generated ones from each state in a hand (the
play the engine scores most, a random play, a random discard, seeded by the state's name),
which reach plays the owner didn't make. Against the harness it checks each hand's score
(`handlog`'s verdict: a roll lands between every roll failing and every roll hitting) and the
state `Board::after_hand`, `Board::discard` and `Board::at_blind_set` leave. Every difference
goes in a known group (a roll in range, gap 15's bosses, a step not modelled yet, the owner
acting between two saves, a save written early, card ids, Ramen's rounded float) or is listed
as new, and the test fails on a new one; a summary is written for a before/after `diff`. It
runs where a change touches the engine or a `Board` step the simulated round plays, and after a
game update (CLAUDE.md step 4).

First run, 2026-10-23, on the whole history (6,270 states, 9,298 jobs, 11-12 minutes on 6
runners at nice 19; the feasibility run that led to D17 gave the same, within its own random
plays): the harness matched the game's next save on 2,971 of the history's 2,984 steps (hands
861 of 862, round-winning hands 139 of 140, discards 673 of 674, blind setups 571 of 576,
cash-outs 134 of 139, shop rerolls 150 of 150, pack openings 443 of 443); the other 13 are the
owner acting between two saves (8), the game's save written early (2), card ids (2) and Ramen's
rounded float (1). The engine: every one of 5,594 hands scores the same or in range (604 with a
roll); `after_hand` leaves the same state after 4,235 of 4,298 hands (the rest rolls in range,
a Glass card breaking, which no step does (18), and Crimson Heart (6)); `discard` after all
2,305 discards; `at_blind_set` sets 532 of 576 blinds the same. The 44 are the jokers'
`setting_blind` context besides Burglar (22: Ceremonial Dagger eating its neighbour, Riff-raff's
jokers, Madness), bosses gap 15 plays as plain blinds (21: The Pillar and Verdant Leaf 17, Amber
Acorn's shuffle 3, Crimson Heart 1) and a Juggle Tag's `round_start_bonus` (1): not modelled.
Since 2026-10-27 (The Pillar and Verdant Leaf simulated, gap 15): `at_blind_set` 548 of 576,
the 28 left the `setting_blind` jokers (23, one of them a Pillar setup), Amber Acorn (3),
Crimson Heart (1) and the Juggle Tag (1). Since 2026-10-27 (the round's start: Chicot,
Riff-raff, Marble Joker and the Juggle Tags held set as a blind is): 549 of 576, the 27 left
Ceremonial Dagger and Madness (18, not modelled: gap 8), Riff-raff's random jokers (5: as many
made as the game's, other keys; none of 256 seeds drew the game's), Amber Acorn (3), Crimson
Heart (1); nothing new. A
round won changes more that no step does (983 hands in the feasibility run): rent and Gold cards'
money, Popcorn, Rocket, Gros Michel's end, bosses' debuffs lifted, all valued outside the
round. What the history can't exercise: 39 of the 150 jokers never owned (Glass Joker, DNA,
Hiker, Midas Mask, Bloodstone, Brainstorm, Perkeo…), Violet Vessel, consumables used, buying,
selling, skipping.

The bot bench (`tools/botrun.py`, through BalatroBot in the real game on the bot's own profile)
plays whole runs with the advisor choosing. It isn't a check in the workflow: a run measures
the advisor only where the bot follows it, and the bot doesn't follow tarots,
vouchers, Arcana, Spectral and Standard packs, "use X first" or skipping a blind (its docstring
keeps the list; each run records the advisor's top choices it couldn't take, `not_followed`).
What was measured on 2026-10-23/24, at 1904f2d (Red Deck, 100 simulations a board): a run on the
same game seed with the same advisor seed is the same run (10 seeds played twice, every pair
identical), so before and after can be paired seed by seed and re-run when needed rather than
kept; another advisor seed (a placebo) flips the near-ties and the run goes its own way (seed
BAVGOLDA: a first discard rated 99.4% against 99.6% left $4 less, no Popcorn, a loss at the
Ante 1 boss against Ante 3); a Gold Stake run takes 1-21 minutes (most 3-9) and one White Stake
run to the Ante 8 boss 31, 91-97% of it the advisor's analysis, so a faster game or a simulated
one saves almost nothing. Measured on a bot that skipped every pack (below), so not a level: the
same 10 Gold seeds reached Antes 1-5, mean 3.5, no wins. Five faults of the bench, found and
fixed in that time, voided every run before 2026-10-24 (`void` in `runs.jsonl`): the bot's
Steamodded doesn't save while a pack is open (its `fixes.toml` guard, lifted in the bot's copy),
so the advisor never saw a pack's cards and every pack was skipped; the jokers were never put in
the advisor's order; a pick from a pack never sold for a free slot; the game sets `G.GAME.won`
at the end of the Ante 8 boss round even on a loss (state_events.lua `end_round`); and the bot's
profile had 35 jokers locked (gap 38). The runs of 2026-10-01 (an early advisor besides) are no
evidence of over-buying. The bot is improved as a player, not used as a judge: nothing judges a
fix whose effect shows antes later yet. If one is ever needed, paired play-ons would beat whole
runs (from a saved state where the choice comes up, the old and the new choice each played on
with the same game seed, so only the choice differs); whole runs only once the single-decision
gaps are mostly closed and the bot follows most of the advice (`not_followed` near empty), and
run around the clock through the D17 harness, headless, so the owner can play alongside.

## Patterns

The engineering practices behind the stages, in plain words:

- **One source of truth.** Each idea is defined once and reused: what a run gains (`Gain`),
  what it's worth (`LongRun::value`, and in this ante's blinds ahead `Ahead`), a planet
  (`LongRun::planet`), held money (`Spending`),
  how options are compared (`compare::race`). Two formulas for one thing will drift apart.
- **A pipeline of stages.** Moves tried → simulated player → valuation → noise. Each piece of
  code belongs to one stage and each fix to one stage.
- **Search, then narrow.** Generate every candidate and let simulation discard the bad ones;
  never rely on a list of moves someone thought of. Where there are more than a race can take,
  a generator over every card by the game's own rules (a superset of a defined class, checked
  against every candidate on small cases) gives them, and any cut beyond it is a budget named
  in the register and in the output (D10).
- **Assumptions are parameters.** A number nobody can derive is a named value in one place,
  listed in the register, labelled in the output, and replaceable by a measurement.
- **Calibrated words.** Outcomes are chances, not verdicts: no certainty words ("easily",
  "100%") unless the chance is at least 95%. Advice that sounded sure and then missed is
  worse than a plain number.
- **Deterministic.** Same input, same output: fixed seeds, and nothing depends on the order
  cards or options happen to be listed in.
- **Regression-proofed.** Every state where the advice was caught wrong becomes a replay
  fixture; restructurings must reproduce the output exactly; behaviour changes are reviewed as
  differences.
- **Fresh eyes.** A reviewer that hasn't seen the reasoning (`advisor-reviewer`) checks every
  change to how advice is decided or valued, and checks the fixes to its findings.

## Rules for changing the advice

1. **One measure.** Every value of money, planets, jokers or cards goes through
   `advise/value.rs` (`Gain` → `LongRun::value`, `LongRun::planet`, `Spending`; in Best play's
   round also `Ahead`, what it does in this ante's blinds still ahead). A new kind
   of gain is a new field in `Gain`, not a new formula somewhere else.
2. **Search, don't pick.** A new kind of decision generates all its candidates and lets
   simulation narrow them down. Where they are more than the race can take, they come from a
   generator that covers every card by the game's own rules (a superset of a defined class,
   checked against every candidate on small cases); any cut beyond that is a budget named in
   the register and labelled in the output, never a cut by the cards' order (D10).
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
   snapshots identical for a restructuring, differences explained for a behaviour change; the
   picks a change moves on the owner's saved states judged by the oracle (D12); the slow checks
   where they apply; `advisor-reviewer` on the change and on the fixes.

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
| Rent $9 an ante | `value.rs` `RENT_PER_ANTE` | $3 a round × 3 (card.lua `Card:calculate_rental`, at each round's end) | exact |
| Selling a joker now (the shop's sell option): its rounds without it (the contribution pass), its sale now, the run with it let go its better way (`sold_now`: replaced or sold outright; one that doesn't last gone by then anyway); where the projection lets it go anyway, the one round it would be held for until the next shop, which decides again (`round_flow`: its rent, less its money on that round and what it pays besides, Rocket's raises, Egg's sell value, To the Moon's interest, a round's as `floor_for` counts its rounds; `floor_for` holds a joker bought about an ante), not the rounds until the projection's let-go (so the rent a perishable pays until it runs out, or a perished one until it's sold, is that one round); a joker that runs out leaves its slot to the stand-ins (a find assumed then), one sold outright leaves it empty (being replaced is the find); leaving wins a tie with a sale (D8: leaving keeps the joker), so a let-go joker's round of rent, where it's inside the 3% band, is shown tied, not taken (gap 7). The projection's own keep-or-let-go (`LongRun::build`, `Fate`) judges each joker alone against your board with all kept (let go where that leaves no less: replaced in its best slot, or sold outright, whichever leaves more), not jointly; what a joker it lets go nets is money once: its sale, less a typical find's price where one replaces it (`find_price`: the shop pool's prices, each joker by its rarity's odds, 70/25/5, over that rarity's count; every joker the game has when the pool isn't known); on a sale's board the projection's and the sale's money go through one `once` (the sale against leaving told exactly), on another option's board apart from the option's (`once` of each, not of the sum: interest steps move it either way); for a joker let go anyway, the sale isn't counted again in what money past the cap buys (`Spending`) | `value.rs` `sold_now`, `round_flow`; `advise.rs` `analyze`, `rank_options` | holding it one shop more | a standalone sale against holding it, played on (`tests/shop_playon.rs` plays buys, not a sale alone, and no rent, yet) |
| A joker's money: the money a round won pays on the board the projection holds (the jokers kept, one replaced by its typical find, a joker bought, a `Gain`'s round) against your board, and your board against it without the jokers the engine pays money for (`engine::PAYS_MONEY`; so a copy counts once, with the joker it copies), each the mean difference on the rounds both boards win (`sim::RoundResult::cash`: scoring, discards, the round's end with every joker's `calculate_dollar_bonus`, a hand left at `money_per_hand`, a discard at `money_per_discard`) of 32 of each of this ante's three plain blinds (`MONEY_ROUNDS`, `advise::plain_rounds_of`: the rounds `value::Growths` plays, by the simulated player without the round's goals, round targets drawn each round), the same deals for both; counted past $0.5 an ante (`MONEY_NOISE`, the deck path's threshold too) and 2 standard errors of that mean (`MONEY_CLEAR`), 3 rounds an ante, as money held every ante; plus what the jokers pay besides a round's play: Rocket's raises, the mean over the rounds ahead of those paid by then ((antes − 1)/2 + 1/3: the plain blinds measured have no boss; a boss round won pays its own raise in the engine), Egg's sell value a round as money, To the Moon's interest at your money now. Not modelled: Matador (pays on a boss's rule; the plain blinds have none), Gift Card (others' sell values), Credit Card, 9s added later for Cloud 9 and planets used later for Satellite, interest on the money a joker brings; the empty slots' stand-ins earn none | `value.rs` `income`, `money_board`, `money_difference` (whatif's too), `added_money`, `payout_besides`, `round_money_now`, `MONEY_ROUNDS`, `MONEY_NOISE`, `MONEY_CLEAR`; `advise.rs` `plain_round_cash`; engine `Joker::round_end_dollars`, `payout_after_bosses`, `sell_value_a_round`, `interest_added`, `PAYS_MONEY` | money as the round pays it, at this ante's rate | noise: the same measure on more rounds; the rounds won only: a lost round's money with Mr. Bones' save |
| 48 rounds per projected board | `value.rs` `long_score` | projection noise | compare against more rounds |
| Growth by Ante 8: a reroll an ante done anyway; a pack skipped only when the budget buys it (skipping gives up what the pack holds, so it costs a pack: $4), at most the 6 packs an ante's shops hold (2 a shop, game.lua); rerolls at the game's price (+$1 for each before it in a shop; one-off money buys them at the first price), at most 5 bought an ante; packs from skip tags are counted in skip-or-play; every joker that grows from an event grows; what no round or event changes by a per-joker table (Popcorn, Hologram, Constellation, Madness, Campfire). The dig list's growth label (and next-ante reach) counts only the reroll done anyway; what spare money buys is By Ante 8's | `value.rs` `fill_long`, `events_per_ante`, `PACKS_PER_ANTE`; `advise.rs` `grow_antes`, `grow_one_ante` | how growing jokers grow | logged runs |
| Growth in rounds (the jokers a round changes) measured on 16 runs of rounds (`GROWTH_CHAINS`), 3 an ante to Ante 8, at this ante's plain blind sizes in turn (×1, ×1.5, ×2 the base: no boss's rule or size), from your board as it is, by the plain player (it plays to win, not to grow them), then yours again (growth and whether they last) by the player playing to the round's goals as Best play's are made (a second pass since 2026-10-17, on the same deals: `LongRun::round_goals`, a dollar without what money held buys; their worths from a projection that keeps them however they project, linear without the floor at losing them, used up as their numbers at none; one pass, not iterated to a fixed point); a shop joker's by the plain player (the same joker owned and in the shop can project apart); the second pass costs 0.07-1.7 s an analysis with such jokers (2026-10-17, most at late antes); a joker used up in at least half the runs is gone; a difference now carried whole (resets not priced); a lost round doesn't end a run, a save is spent in it (the Mr. Bones that saved is gone for the rounds after) | `value.rs` `Growths`, `GROWTH_CHAINS`, `LongRun::played_for`; `advise.rs` `growths_for`, `played_for`, `growth_rounds`; `sim.rs` `grown_over`, `JokerGrowth::numbers` | how the run's rounds grow them | more runs; the save's own counters (`unused_discards`, rounds played) against the runs' |
| Madness horizon 1 ante | `value.rs` `long_of` | it eats your jokers | — |
| Spending: level cap reached at the interest line + $30; packs $4 (`PACK_PRICE`), 2 a shop, each further one ×0.8; typical pack by shop weights (game.lua 4/4/1.2/0.6/4), Standard counted as 0 | `value.rs` `Spending` | money past the cap buys rerolls and packs | logged packs opened and picks |
| The money's walk: 2000 walks, the most rerolls a shop (up to 8, `WALK_REROLLS`) chosen on half of them and valued on the other half (`WALKS`); a find in a free slot or, the slots full, in place of the joker of yours (not Eternal or Negative, the one the option buys among them) or the one found before whose loss costs least, whichever leaves more; the jokers kept, and the board an option leaves, as score gains added (each joker's multiplier less 1; one of yours replaced loses 1 − 1/its multiplier), read off your board's scores in that round as a lognormal with the spread of its p10 and p90 (at least 0.05 in log terms), placed along the scores at its simulated chance (at none or a sure pass no nearer the target than half a round in its count and its median allow, `Scores`); each joker's and each option's multiplier the one its own simulated chance gives there, under 3 wins its mean against yours but no more than 3 wins' (`Scores::ratio_for`); never under the best one's own simulated gain; By Ante 8, reach and next-ante strength values the best, each further buy ×0.6, a joker of yours replaced losing its whole gain | `advise.rs` `money_value_with`, `Kept`, `Scores`, `Start` | what money buys in the shops ahead; joker gains don't add up as chances | **measured** against the same money played on (`shop_playon.rs` `money_finds`, the judge's leaving with up to 8 rerolls a shop, "spend8", on the 25 shops in `tests/fixtures/private/money_finds_states.txt`, 2026-10-27): this boss 0.018 high (mean absolute error 0.023), the next ante's boss 0.021 high (0.046), against the old sum's 0.036 high (0.044) and 0.062 low (0.128); the gains multiplied stack small finds on a hopeless board into a near-sure pass (gap 17) |
| A dollar in Best play: what +$10 does, taken as linear | `play.rs` `dollar_gain` | money won this round | — |
| Comparison: at most 1,600 rounds an option, "clearly worse" at 95% (paired; where a round can be lost outright, as in Best play, with one round in its favour worth the largest round value counted as if it had happened: what a parting round not yet seen could make up, about 3 × the largest round value / n with no spread, the rule of three as for a tie; a graded measure, a deck's score ratio, as it is), "equal" within 1% of the leader's value | `compare.rs` `MAX`, `EQUAL` | when a difference is real enough to act on | — |
| Best play's search budget: first batch 16 rounds (batches double); at most 48 undecided moves after 16 and 32 rounds, 24 after 64, 12 after 128, 8 after that, and with many moves at least half of them for each doubling instead (`compare::halving`: an 8-card hand's 436 moves, 218 after 16 rounds, 109 after 32, 55 after 64; since 2026-10-11, before a first batch of 32 rounds cut to 48: below), kept by how little the rounds show them worse than the leader (Noise stage; until 2026-10-05 by value, which on few rounds kept a steady money edge over a move a rare lost round would separate), on exact ties by value, then how close lost rounds came to the target (a first cut at 16 rounds, to a few moves by value, ranked moves on money before a rare lost round could show: the safest discards fell out; the cut at 16 rounds now keeps half, by the bound that counts the unseen round) | `advise.rs` `SEARCH_FIRST`, `SEARCH_BUDGET`; the halving factor 2 (`compare::halving`) | cost vs. coverage (2026-10-11: the rounds raced on 101 of the owner's states +13%, Antes 4–6 +20–31%, mostly near-hopeless rounds that before stopped on a false "equal"; a round no move can win races to the cap, 2.2–3.1× the rounds on three fixtures made unwinnable, 1,710 moves with a Cryptid 54.7k → 117.8k) | `search_against_reference` (2026-10-03, seeds 42-44, the 6 fixtures in a blind; a pick passes when gap − 2·se ≤ 1%; 2026-10-11, 11 fixtures: every pick passes) and the history check: see known gap "search misses" |
| Best play on a hand of more than 12 cards (`ALL_MOVES_MAX`; every play and discard of 12 is ~3,200 moves): its structure's plays, each also topped up with up to the 5 least-worth cards outside it (plain first, then chips), the best 8 of each kind (hand, cards played) and then the best by score now with every roll failing, 256 in all (an 8-card hand has 218 plays), plus the structure's discards (D10); the first 64 cards, the first 10 Stone cards | `sim.rs` `ALL_MOVES_MAX`, `BIG_HAND_PLAYS`, `BIG_HAND_PER_KIND`, `big_hand_moves` | cost vs. coverage (32 cards: tens of thousands of plays from the structure) | a big-hand state against a reference with every play the structure gives |
| A tie (two options within 1%) takes n shared rounds with 3 × the largest round value / n ≤ 1% of the leader's value (about 300 when a round is worth about the leader's value): a round where they part ways may be rare, and when none has turned up in n rounds it may still happen 3 in n (95%, the rule of three); and a round worth something seen (on rounds all worth nothing, a round you rarely win with none won yet, the round that would part them could be worth anything: since 2026-10-11) | `compare.rs` `paired` | the rule of three | — |
| Which hands carry your points (the main hand planets go to, joker growth, the style outlook): the hardest round of the ante, played from the start (a round in progress begins again, with the blind's own rules) | `advise.rs` `hand_mix`, `fresh_round`, `blind_from_start` | your hands in a typical round of that blind | — |
| Simulated player's policy: a dig plan (`aims`: a flush of a suit you hold 2+ of; a straight you hold all but 1 or 2 ranks of, with Four Fingers and Shortcut; a straight flush by the game's rule, a flush part and a straight part found apart (with Four Fingers each is 4 of the 5 cards played): for each suit you hold need − 2 or more of (Wild and Smeared counted) and each straight, the suit's copy of each rank, another only within the 5 − need cards a play may have off the suit, plus the suit's other cards while there's room; one card away, its fits are the deck's cards that make a straight flush with them (`hand::detect`), two away a card of each missing rank, as many of them of the suit as the flush part lacks (the likelier way when either can be); one more of a rank you hold 2+ of; a Full House from Two Pair; one more of a single card's rank beside a Pair (Two Pair) or Three of a Kind (a Full House), with one or two singles (`AIM_SINGLES`), those with the most cards of their rank left first) chased dig by dig (`chase`, worked out from the last dig back: a later dig is spent while the chase from there beats playing the hand the first dig leaves, the same at every dig), when the points the rest of the round is expected to score with the chase beat playing on (the best play now, then a later hand played on each hand after (`playing_on`)), on pace by more than half a best play over playing on (`ON_PACE_MARGIN` = 0.5; 0.25 measured within noise of it; then only one card away and when playing the best hand would spend 2+ of its cards); at each dig the plan is made with its chance given the digs before failed (odds exact for a flush, `flush_odds`, else the cards the digs would see as one draw, `draw_odds`: exact for one card away when every other card is thrown each dig; the cards held back for their held value take slots a dig would refill, and a plan whose cards don't fit beside them has none (since 2026-10-16), and a straight two ranks away counts only its likeliest pair of ranks), else a lesser plan whose cards it keeps (a straight or flush on the way to a straight flush) ends it there when it puts the round on pace (its chance per dig: its cards among the dig's new ones; valued on its full estimate, and when the score reads held cards in the chasing plan's hand: beside its other cards and what its dig leaves, `lesser_of`), else the chase goes on or stops with the hand it leaves (the best play of the hand the first dig leaves: all but the cards its discard or junk hand takes, `dig_for`; cards worth holding stay); a hand made is played once, the hands after it at a later hand's average (`playing_on`); a dig made with a junk hand costs that hand (its own points aren't counted), and under a press-play discard (The Hook) goes on only with the chance the discard spares every card the plan keeps among the cards the junk hand leaves (its own, those kept for their held value, the rest of up to 5 not played), else the round plays on from the hand it leaves; a later hand, made, failed or stopped, is worth a later hand of playing on the hand you hold now (`later`, from `playing_on`: what the jokers grow or fade by and the round's memory count, but not The Eye's ban on the hands played since, the cards are taken to come back); a lesser plan ends the chase when its score every hand left keeps pace (the flat pace: the made hand's own growth isn't worked out); its score the average over its completions, each played beside the cards the hand holds then when the score reads held cards (what the dig leaves and cards drawn into the rest, `held_beside`, `HELD_DRAWS`; the hand the first dig leaves the same: since 2026-10-16, before only the cards kept for their held value, so Raised Fist read nothing beside a completion and Blackboard always paid) (`aim_completions`: every distinct card that fits, by its copies, when one is missing from each group, else 4 seeded random draws; a fixed typical card was biased both ways: neighbouring middle ranks made flush draws straight flushes, spread ones never did), screened on its middle completion, the best 2 in full (`AIM_FINALISTS`); duplicates (Smeared Joker) once; a straight keeps the copy of a rank in the suit you hold most; a straight two ranks away only when no straight one rank away keeps the same cards; the play itself chosen by its floor (every roll failing, a pick of cards such as The Hook's averaged over what it could take, gap 15; whether it surely wins by the pick that costs most, `Play::sure`; with the round's goals, a winning play by what the round is worth, `finish_play`, and with no win on the table, a play that grows the jokers more when it costs no pace, `spare_instead`, gap 18), pace and the chase on its average over 8 fixed rolls (`MEAN_ROLLS`); cards that add to the best play while held never thrown away, nor played in a dig (under The Hook, measured against a card drawn from the pile in their place over the sets it could take, gap 15; else left out; the keep is all or nothing: the drawn-card measure on every board and a keep weighed against the dig were measured and rejected, gap 15); discards that pay money (`engine::discard_money`: Mail-In Rebate, Faceless Joker, Trading Card) cashed while on pace (a threshold, not yet weighed against `RoundGoals` money), the best-paying one whose remaining hand keeps pace, at most 16 tried, the last one right before the round ends; discards burned first when the chosen play scores > 0.1% more with none left (`burn_pays`, by discards left only: what the discards do to the jokers isn't weighed, gap 8; which cards, the policy's own dig choice) | `sim.rs` `decide_cards`, `aims`, `chase`, `aim_odds`, `aim_score` | how a player digs | `tests/player.rs` (2026-10-04: 50.1% → 62.5% at the fixed targets, 2026-10-04 straight flush plans: 63.75%, 2026-10-05 chases dig by dig: 65.65%; 2026-10-10 the last hand digs while a discard is left (button_callbacks.lua `can_discard`): every 4-hand round 61.84% → 63.19%, none worse, then Two Pair and Full House plans from a Pair and Three of a Kind: every round 59.52% → 61.03%, none worse (the Spare Trousers boards +13 to +20), The Needle's one hand with 3 discards about 4% → 52% (targets set by that player), the gap to the oracle 11.0 → 5.1 points over 74 cases (boards and fixtures, 200 rounds); on fresh rounds against the version before, +1.94 ± 0.10 points (seed 1234, where the on-pace margin was chosen: 0 +1.22, 0.25 +1.67, 0.5 +1.71, 1.0 +1.53, never chasing on pace +1.15, before the hand a chase leaves counted the cards worth holding) and +2.03 ± 0.10 held out (seed 5678); against the oracle; D9); replay fixtures; calibration of win chances |
| Cards held beside a dig plan's completion, and beside the play of the hand its first dig leaves, when the score reads held cards: the plan's own cards beyond the five played, the cards worth holding, then the rest the dig leaves, up to the hand's size (a guard: a plan that doesn't fit beside the cards held back has no odds), the slots left drawn from the pile; the same 4 draws for every plan and the hand a dig leaves (each completion one draw at least, every draw used, though not equally often: with 3 completions two of the 4 count twice; one order of the whole pile per draw, the plan's own fits skipped); the screen (`aim_quick`) on the first draw only | `sim.rs` `HELD_DRAWS`, `held_beside`, `held_draws`, `aim_quick` | the hand a plan is completed in | `tests/player.rs` and paired rounds (2026-10-16: 16 draws for the completions in place of 4 moved no board beyond noise, ±1 point on 2,000 rounds); exact over the pile's kinds would remove the sampling; the screen on every draw would remove its one-draw bias (Raised Fist reads the lowest of about 3 drawn cards) |
| A Two Pair or Full House dig plan keeps its group and at most 2 single cards beside it (each another rank that completes it, one card fewer drawn per dig); the best of a group's plans only gets the full estimate (`Aim::family`) | `sim.rs` `AIM_SINGLES` | how many draws a plan gives up for more ranks that complete it | `tests/player.rs` (2026-10-10, every round: 1 single 60.97%, 2: 61.03%, 3: 60.73%; with three the plans overrate themselves, since a pair among the cards drawn isn't counted); counting every card that makes the hand (`hand::detect`) would let the chase weigh it |
| Playing on's later hands (`playing_on`) search only the plays a full search left unbeaten: of each hand type, those no other play of it beats in both chips and Mult, before a deck balances them (what the board one hand on adds to every play, +Chips, +Mult, ×Mult, keeps a beaten play beaten). The search is the decision's own of the hand (hands of up to 12 cards, `HandParts`; else the first later hand's); searched in full again after a hand's step that moves plays of one hand apart (`Board::after_hand` says: a hand's level changed, The Arm, whose Mult comes before the cards' ×Mult; a joker gone, Ice Cream, Seltzer; the last hand reached with a joker that retriggers played cards, Dusk). Missed: a joker effect that differs by play scored after a joker whose numbers moved, unless both add (a ×Mult that differs by play after a growing +Mult: Flower Pot, Seeing Double, The Duo to The Tribe; a +Mult that differs by play after a growing ×Mult: Jolly to Crafty, Half Joker after Obelisk, Vampire, Loyalty Card) | `sim.rs` `later_best`, `front_of`; `Board::after_hand` | cost vs. coverage (2026-10-12: searching every later hand again was ~63% of a late-ante shop analysis; a top 2 of each hand type instead missed 27 of 9,000 hands of enhanced cards, up to 17.5% of the total; under The Arm no later hand is spared a search) | against searching every later hand: held out (other seeds, 5 hands, hands of 8 and 10, 16 growers, faders and retriggers beside every joker, five bosses), 0-3 of 24,000 totals differ, worst 1.6% low (with the Plasma Deck 12-15, worst 1.3% low); on two Ante 7 shop analyses 1 of 1.3M decisions' pace totals, 0.7%; `the_later_hands_front_finds_what_searching_them_all_finds` (27,000 hands, the grower after the joker over 4 hands and before it over 6); the player benchmark (every board's win rate and oracle gap unchanged) |
| What a round leaves the jokers it changes (`engine::CHANGE_IN_ROUND`: Green Joker, Ride the Bus, Runner, Square, Spare Trousers, Wee, Lucky Cat, Vampire, Obelisk, Ice Cream, Seltzer, Ramen, Castle, Yorick; each told apart from its copies by its place on your board, `Joker::id`): its Mult, ×Mult and Chips as they'll stand after the round (`Joker::lasting`: Hit the Road's ×Mult back to ×1) against where they were when the analysis started, each at what one more adds by Ante 8, measured on a step of +2 Mult, +×0.25 or +10 Chips (`GROWTH_STEPS`, about a hand or two of the growers' own; only on the numbers its measured rounds move, `Growths::moved`) and taken as linear, never below nothing per unit, several jokers' as factors of the run's value multiplied (`sim::Grown`: since 2026-10-18; added, falls past a round floored a won round at nothing); a fall never costs more than losing the joker, which is its place to the ×1.25 typical find it's kept against (`Fate::Replaced`; not in the growth runs' second pass, `JokerWorth::floor`, where it would make every fall below a typical find free); used up (Ice Cream, Seltzer, Ramen), that loss. Nothing for a joker the projection drops (a perishable that runs out) or replaces, nor for one debuffed. In this ante's blinds still ahead also the counter a joker's ability keeps that a round moves, apart from its three numbers (`Joker::counter`: Loyalty Card's place in its cycle, Yorick's discards to its next step, Seltzer's hands left; next row), not by Ante 8 (Loyalty Card's cycle turns many times by then and Seltzer runs out, `LongRun::lasts`; Yorick's count is progress to its next ×Mult step, not counted there: the discard that crosses it gets the whole ×1, gap 18). Not counted: what a hand does to your cards (Vampire's enhancements, Midas Mask's Gold, Hiker's Chips, Glass breaking), hand levels it changes (The Arm, Burnt Joker, Space Joker), the game's own counts a round moves that jokers read (each hand's times played: Supernova's Mult, Obelisk's most played hand), consumables used in the round (Constellation: holding the planet grows it later too) | `value.rs` `joker_worth`, `GROWTH_STEPS`; `sim.rs` `RoundGoals::jokers`, `JokerWorth::of`, `grown` | what a joker's growth is worth | 2026-10-14: Square at 28 Chips (Ante 2) 0.0037 of a won round a Chip, Spare Trousers 0.0048 a Mult, Green Joker 0.019 a Mult, Obelisk 0.55 a ×Mult; linearity not checked (steps of half and double size would show it), nor the steps against memoised exact end states |
| What the board a round leaves does in this ante's blinds still ahead, for Best play's round (`value::Ahead`): the Big Blind's (if still to come, taken as played) and the boss's played one after another on the analysis's simulation count of runs (`sim::chained_runs`, 300, the player without goals, each round on its own stream), each from the board the one before ends with; every board played carrying the shops' find (`Ahead::find`): every score in a blind ×x (its target divided by x: it takes no slot and no joker reads it; outside Plasma Deck the same as a ×Mult joker scored last), sized shop by shop where the shops' estimate is measured, on the boss alone from your board as it is (`boss_runs`): the first shop's on every blind ahead, sized so the boss gets through as often as its chance as it is plus one shop's lift, then each next shop's, where it adds to the lift, on top of it from the blind after that shop, to that many shops' lift (the boss's chance with them by the money you hold, `money_value_with` as `survival_with` counts it, less its chance as it is: the shop ranking's estimate of what money buys (on your board as it is, with no joker of yours to swap and, on a full board, one free slot it doesn't have: gap 17); the boss's estimate with one shop stands for the Big Blind's, whose pool isn't measured; at most 3 runs short of all, the rule of three, `shops_target`; none in the quick pass, which screens no pool); then, where your board carrying it gets through these blinds less often than 10%, every blind raised by one more multiplier until it does (`AHEAD_FLOOR`: the shops' estimate counts only the jokers money finds, so a board it leaves near none meets the blinds stronger; with the rule of three's 3 wins instead the same change was worth 9× more on 300 runs than on 60); each multiplier found by a search over ln x against the log-odds (`size_find`: from ×2, then each step's slope, at most ×10 a step, ×10 after a step that moved nothing and two such steps in a row end it, then the secant kept a tenth inside the two either side; stopping within 2 standard errors of the chance; at most 6 sets of runs a search, `FIND_STEPS`, and ×1000, `FIND_MAX`: a budget, the closest tried kept, the smallest of those as close; named in the output; it also gives the points it tried, `Curve`); each goal's change as the shift of the log-odds between the chances it was measured at on those runs (chances of none or all taken half a run in, `sim::log_odds`), counted only where the paired runs show it either way (`compare::better_by`, the paired difference less 2 standard errors), placed at the chance it was measured at (the find board's through these blinds, the reference); the shifts of what a round leaves added (odds multiply) and the chance they give taken against the reference (`RoundGoals::value`: between none and a sure pass; with no shops counted and a board at 10% or more, one change alone is the share it was before); a joker's numbers stepped up, down a step of the same size and down to none (`sim::AheadWorth`), linear in the log-odds between them, and the joker taken away, kept apart from its Ante 8 worth (the two parts multiplied, `sim::Grown`); a joker's counter a round moves (`Joker::counter`) at each value the round in play can leave it at (`Ahead::counter`: `Joker::counter_after` with its hands and discards left, at most 5 cards a discard and The Hook's 2 a hand), each a shift against the counter as the analysis started, gated as the numbers are, added to its numbers' shift (`sim::CounterWorth`), at most `COUNTER_POINTS` (8) values measured, every so many of the values reachable in the order the fewest hands and cards reach them, the first and last included, the rest linear between them by value (a budget: Loyalty Card's 6 and Seltzer's hands fit, Yorick's up to 23 are read between); a Blue Seal planet as a level more; a dollar by what $10 more buys in the shops before each blind (`survival_with` with $10 more, `Ahead::dollar`): the shift between your board carrying the find and carrying the find for $10 more, a tenth of it, linear in the log-odds; the find for $10 more raises each blind's multiplier so the boss, carrying the find for the shops before that blind, gets through as much more often as $10 more adds to those shops' lift, from where the find put it (the difference of the lifts: sized to the lift with $10 more on its own, the search's 2 standard errors take a typical $10 lift of 0.03-0.05 to nothing or twice over), at most the rule of three's 3 runs short of all (`shops_target`), never less on a blind than on the one before; with one blind ahead the chance it gives is that (a search for it only adds its tolerance), with two measured on the runs (`find_10`), each multiplier read off the points the find's own search tried for that blind (`Curve::raise`: linear up to the first tried past that chance, so never past a multiplier it saw get there; else the slope across its last bracket; a stage that leaves the boss's runs as they were, a shop that adds nothing or a search that found it there already, keeps the points of the one before), and where they don't reach it (none tried, or no bracket) or `AHEAD_FLOOR` binds, a search on the boss carrying the find, the floor's too, within one standard error of the difference on the same runs and at least a run (from those points' slope, or the floor's where it binds; unseeded, from ×2, where no search tried any; a seeded search's step that moves no run keeps its slope); not gated by the paired runs: the runs size a change whose being there is the shops' estimate's (gated by its rule of three, `measured_p`), as the shops' find itself isn't gated (on the same runs a 0.04 lift the find put on the boss was taken to nothing about half the time) (none in the quick pass); several jokers' Ante 8 factors multiplied (not measured: the evidence below is the blinds ahead's); a factor at least a millionth (`sim::LEAST_FACTOR`, so its log stays finite where a fall or a loss costs the whole run); a card worth drawing only once its Ante 8 worth passed (a budget), as a share of a round added to its Ante 8 worth. Not counted: the next ante, a skipped Big Blind, the game's other round-end steps between the blinds | `value.rs` `Ahead`, `AHEAD_FLOOR`, `size_find`, `FIND_STEPS`, `FIND_MAX`, `Ahead::counter`, `COUNTER_POINTS`; `joker.rs` `Joker::counter`, `counter_after`; `sim.rs` `chained_runs`, `AheadWorth`, `CounterWorth`, `Grown`, `log_odds`, `JokerWorth::of`, `RoundGoals::value`; `advise.rs` `Ctx::ahead`, `survival_with` | what surviving this ante is worth | 2026-10-17: `ride_the_bus_queens_dig` (The Pillar played as a plain boss, gap 15): Ride the Bus +0.075 of a round a Mult above +7 (the Big Blind borderline), the reset costing the Queen plays their place. 2026-10-18, against the joint runs on 43 saved Small and Big Blind states with two or three jokers a round changes: several jokers reset at once, added as shares off by 0.86 and 0.32 of the chance's share on average (19 of 21 below a lost round), as log-odds 0.028 and 0.016; all a step up, added 0.13 and 0.11 (14 of 19 past a sure pass), log-odds 0.020 and 0.060; five steps up from one, linear in the share 0.51 off, in the log-odds 0.12. Moving a change measured on the runs to a stronger board (100 resets measured again with a ×1.25 to ×3 stand-in joker added): where the stand-in lifts the chance modestly (×1.25, ×1.5: 65), the same log-odds shift off by 0.13 of the chance there, the same share 0.13, the same change of chance 0.31; where it dwarfs the joker reset (×2, ×3: 35), every rule 0.36-0.41; on boards the runs leave near none, the reset loses all the stand-in added there, which no measure on the runs can show. The shops' lift, 14 sampled Small and Big Blind states (full pass): the boss's chance 0.37 → 0.98, 0.72 → 0.98, 0.16 → 0.39, 0 → 0.25 with the money held, 6 of 14 lifted by more than 0.2. 2026-10-18, the change measured on a board carrying the shops' find against transported from the runs without it: on 47 saved Small and Big Blind states with two or more jokers a round changes (and `ride_the_bus_queens_dig`), 35 where the shops lift the chain's chance (by 0.02 to 0.79), each grower's step up, step down and reset (209 changes, gated as the code gates them): the transported share off by 0.084 of a round on average from the same change measured with every score ×x and 0.088 from it measured with a +Chips stand-in joker as the find, while those two finds differ by 0.039 (gate disagreements 63 against 26); transport counted a step up 30% higher (0.079 against 0.060 on average) and a step down 20% dearer; on a board at 1% lifted to 64% (Ante 7: Ride the Bus, Green Joker, Ramen) it counted nothing for resets that cost 0.80 to 1.0 of the round there, and at 3-5% lifted to 81-84% it gave every losing change the same −0.74 or −0.82 where the find board told an Ice Cream reset (−1.0) from a Square reset (−0.29). The find's shape: a ×x joker scored last agreed with every score ×x within 0.003 (apart from where a joker copying the one beside it took it up); a +Mult stand-in placed last was 0.10 to 0.17 off on ×Mult resets (scored after them, it dilutes them). Sizing it on the chain, by doubling and halving: 4-5 sets of runs, within 0.023 of the estimate on average; the multiplier ×1.05 to ×5, most ×1.25-×1.5 (sized on the chain, a Small Blind's Big Blind took both shops' find, and where the Big Blind held the chain back the boss's climbed to ×1000: it's sized on the boss alone since). The dollar, 2026-10-18, on 40 saved Small and Big Blind states (35 where $10 more lifts the shops' estimate), against the same find for $10 more sized by a search on 1,200 runs, each as the share of a round $10 gives at its own reference: off by 0.021 of a round on average, against 0.044 for the shops' estimate's slope at your board's chance with the lift (the measure before); with one blind ahead 0.0024 against 0.0038 (14 states), two about the same, 0.018 against 0.019 (15), below 30% 0.076 against 0.20 (6: the estimate's slope near none placed at the floor's 10%, off by 0.56 on one board; what's left there is mostly where the find's or the floor's search stopped, e.g. 6.7% on 300 runs against 10.4% on 1,200); with two blinds ahead, the find for $10 more sized by a search instead (within one standard error): 0.020 (in the flat top of the boss's chances, where the find landed up to 2 standard errors high, the search raised the boss by ×1.41 for a few hard runs where the search's points put ×1.13), by the slope between the point kept and the one tried nearest it, unbounded: 0.013 (but resting on a run or two where those points are close); unseeded (a first step of at least ×1.1, more than most $10 moves) a search landed past its target on 6 of 8 tries. The counters, 2026-10-22: every one of the 35 saved Small and Big Blind states with Loyalty Card (seeds `LK1JWUX3`, `A7LD1T91`, the history's only ones; Seltzer is on 125 saved states, 17 of them in a Small or Big Blind, Yorick's 12 all at The Wall or outside a blind) gets through its boss in every run whatever the ×4's hand, so the history can't show a phase's worth; with the boss's target scaled so the board gets through about half the time, the 6 phases moved the boss's chance by 0.35-0.80 and neighbouring phases 0.30-0.62 apart; on 7-hand Burglar boards 0.71-0.98 with the ×4 on hands 1 and 7 against 0.17-0.45 with it once, on 4-hand boards none with it on a 5th or 6th hand (never); on the test board (`the_hands_a_round_takes_land_loyalty_cards_x_mult_in_the_boss`) 0.41 with it on the boss's 4th hand against 0.09 on a 5th. Cost, on 4 saved Loyalty states (quick pass): 4-5 sets of runs more (6-21 before), Best play's goals 0.27-0.91 s → 0.41-1.46 s, its race the same; playing each simulated round's board on through the blinds ahead instead would add 1-2 rounds to each of the race's 12,600-42,800 rounds, about 2-4× the race's CPU, with a 0/1 outcome a round in place of a chance, much noisier. |
| Cards worth drawing this round: for each held consumable that goes on one card (one that takes more isn't counted: a race per card kind costs too much, so a pack card picked for an Empress isn't "worth drawing" later), its value on each kind of card in the draw pile minus its value on the target its search chose in hand (48-round screen, kept when over 1% on the full projection; Blue Seal planets counted); tied to its consumable and gone once that's used; a simulated round counts the best one it draws | `play.rs` `seen`, `sim::RoundGoals::seen` | a better target may come | — |
| A consumable's targets (every one that goes on cards you pick): every target set in the hand it's used from (yours when it's held or in the open pack and a hand is on screen, else 4 sampled hands for one you can buy, pick or hold, 1 for the rest of the pool: noisy, and in a blind it reaches Best play's money value through pack values), plus no target (your deck as it is), sets leaving the same decks counted once, a random effect's outcomes (Aura) each valued on the same rounds and weighted by their chances, chosen by `compare::race` on the deck projection (first batch 16 rounds; at most 12 sets undecided after 16, 6 after 32, 3 after that, kept as in the Noise stage (2026-10-05: the sets' values moved up to ~1%, Hanged Man's long-run 1.249 → 1.236 on the fixtures, The Star found a target worth +0.3% where it found none); at most 128 rounds (48 for a tarot no option takes as its own value: `TarotBudget`), however many sets a big hand gives (not halving like Best play's: not measured); the money a set's cards earn becomes income once, from its first batch, against your deck's money on the same rounds), on rounds of their own (after the 300 the pick is then valued on); no target never cut by the budget and, deciding a use on the hand on screen (the consumable stays held), kept when it's as good as the best (D8; valued on sampled hands, the best set is taken); the sets it can't tell apart from the best are listed (at 128 rounds the race can't show a tie, so these are the sets still in at its cap); the rest ordered by their paired estimate against the best; a deck's extra Blue Seal cards counted as planets (`seal_planets_in`, 3 rounds an ante) | `advise.rs` `TARGET_FIRST`, `TARGET_MAX`, `TARGET_BUDGET`, `value.rs` `deck_rounds`, `deck_value` | cost vs. accuracy (a first batch of 8 dropped the best targets) | check if better targets are ever cut by the budget |
| Tarots and spectrals no option takes as its own value (not in the shop, an open pack or held, nor the one The Fool on offer would copy: `valued_singly`), whose values only go into averages and bests (the packs that hold them, what money buys, `value::Spending`, The Emperor's own value, the Charm and Ethereal tags' packs when skipping a blind) and the list: their change simulated on 60 rounds this ante and the next (`Options::screen_sims`, paired: the change on those rounds against your deck on the same ones, on top of your deck's value on the full rounds), their target race to at most 48 rounds (`TAROT_SCREEN_ROUNDS`; its first batch the full 16), projected to Ante 8 on 100 (`TAROT_SCREEN_LONG`; random outcomes against your deck on the rounds they cover together). Which tarots are screened depends on what's on screen, so a pack's or the money's value can move a little when only another shop slot changed | `advise.rs` `TarotBudget`, `valued_singly`; `value.rs` `deck_value_part` | cost vs. accuracy (2026-10-12: an Ante 7 shop spent ~45% of its rounds on tarots; projected on 48 rounds, Ante 8 values moved up to 0.4 and the best-of-3 money value up with them) | against the full budget, 2026-10-12: 10 shop states (Antes 4–8, 4 with Arcana packs), every option in the same order, the largest moves Seed Money −0.014 By Ante 8 (tied) and Arcana packs ≤ 0.010 and 0.6 points of win chance; screened tarots' own values up to 0.13 off (Ante 8), their mean shift per state −0.009 to +0.002; 6 blind selects with skip tags (one Charm): every verdict the same; ~20% of a shop's CPU (2,868 → 2,243 s over the 10) |
| Random effects (Immolate, Familiar, Grim, Incantation, Aura, Sigil) and the shop's sampled hands: valued over several outcomes (6 for the destroyers; Aura's 3 editions at the game's chances, on the target its search chose), the projection's 300 rounds split between them (however many outcomes: the target search picks on the rounds after them), each outcome on rounds of its own (`deck_value_part`, through `decks_value`), against your deck on the rounds they cover together (100 rounds for a tarot no option takes as its own value: `TarotBudget`; outcomes of unequal chances aren't weighted the same way below, so each slice's luck is left in by its chance less its share); this round's odds for the destroyers come from one outcome | `advise.rs` `RANDOM_OUTCOMES`, `decks_value` | the average outcome | — |
| DNA (a joker that changes a card each round): one sampled opening hand a round (3 rounds an ante; discarding first to dig isn't tried; when it isn't offered now, only in the dig list's pool, one race an ante, its pick made in each of the ante's rounds: a third of the cost, a coarser deck), drawn from the deck with the copies so far, each round's race on rounds of its own, its best set copied as a consumable's is (a set that leads on noise included); what comes round by round is a flow, in the race (`UsePrice`) and the value alike: the hand a copy spends (that round's share of a hand fewer every round, measured once on your projected board, the Blue Seals it sees fewer included: `hand_cost`) and the planets its Blue Seal cards make (from the round it's made); the deck valued as the one it ends with by Ante 8 (as a grower's state) | `value.rs` `round_decks`, `round_effect_long`, `hand_cost`; `advise.rs` `UsePrice` | the cards you'd copy, and when | logged runs with DNA |
| Typical hands with jokers that pay by discards left: scored with your discards and with none, the better counts (only when a reference Pair scores differently) | `advise.rs` `Ctx::typical_n` | you'd keep or burn them | — |
| Look-ahead: 8 futures (one step: they don't look ahead again; seeded from the round's stream without drawing from it, `lookahead_seed`), each line on the same futures, winning now included, an alternative taken only when its paired gain is above 2 standard errors (`paired_pick`, as the oracle; a loss rarer than 1 in 8 can still go unseen), only when it could add ≥ 1% of a won round (`LOOKAHEAD_MIN`): with a win on the table, in the cards that pay at round end (every Blue Seal in hand at the best hand's planet, every Gold card's money, against what the finish keeps) or in growth (the play that doesn't win yet and grows the jokers most, plus what winning now takes off them joker by joker, over a spare hand's money: one play tried, the one that grows most, then keeps the winning play's cards, then scores most); with no win on the table, the highest-floor play that grows the jokers by ≥ `LOOKAHEAD_MIN` more than the policy's (one tried), looked at only when the round played on after it keeps pace and scores at least what it would after the policy's play (`line_pace`: each once, then the policy's best play on the stepped board, `playing_on`: a budget, measured below), on 16 futures (`SPARE_ROLLOUTS`), the second 8 only when the first show a gain (a budget: it only rejects); `sparing_play` is also among the oracle's alternatives | `sim.rs` `play_on_instead`, `growth_play`, `spare_instead`, `line_pace`, `paired_pick`, `LOOKAHEAD_ROLLOUTS`, `SPARE_ROLLOUTS`, `LOOKAHEAD_MIN` | cost vs. depth | `player_values` in `tests/player.rs` (the round's value and win rate on boards with fixed goals); replay fixtures; 2026-10-17 `spare_instead` against the player before (2,000 rounds a board): with the pace alone and no futures the Square board lost 7.6 points of win rate (500 rounds); with 8 futures and no "as much as the policy's play" screen, Square −0.0026 ± 0.0016 of a round, with 16 −0.0008 ± 0.0007 (8 took the 4-card play on noise) but a Square board's quick pass 6 → 20 s; as built, Spare Trousers +0.0119 ± 0.0027 and +0.0181 ± 0.0036 (win +1.1, +1.7 points), growers +0.0006 and +0.0012, Square, seals and seals_growers unchanged; a screen at 80% of the policy's pace instead kept the same gains with growers +0.0019 and +0.0020 but made a Square board's quick pass 6 → 16 s; 2026-10-22 (8,000 rounds a board): the line played on as the pace plays it instead of the same cards every hand, Spare Trousers +0.0031 ± 0.0009 and +0.0035 ± 0.0009 (win +0.27, +0.33 points), the rest unchanged, the Square and seals_growers boards ~35% slower, quick passes on the fixtures as before but `ride_the_bus_queens_dig` 3.3 → 4.0 s and `square_four_card_two_pair` 2.6 → 2.7 s; tested against the policy's line alone, Trousers +0.0041 and +0.0051 but that quick pass 7.6 s; the finish's growth counting what winning now takes off the jokers opened the look-ahead once in 80,000 rounds |
| Consumable used in a simulated round when it lifts the best play by > 1%; one that would put a card that pays at round end in hand (counted on the board after it's used: the slot any use frees doesn't count) is held while the best hand times hands left covers what's still needed (on pace); equal gains by key, not slot order | `sim.rs` `use_if_better`, `finish_with_uses` | when a player uses one | — |
| Verdant Leaf's sale in the simulated round: the joker whose sale leaves the most pace (`playing_on` over the hands left) on the hand held and 8 deals of the round's cards (`LEAF_DEALS`, a fixed order and seed), none when keeping them all scores more; equal ones by money, then key (Best play's moves follow it; it's named on the blind) | `sim.rs` `leaf_choice`, `LEAF_DEALS` | which joker a player sells to lift Verdant Leaf (gap 15) | racing each sale on paired rounds. 2026-10-27, 40 Ante 6-9 boards from the saves played at the Leaf (300 rounds each): 0.012 below the best of every sale and keeping them all on average, 3 more than 0.05 below; the hand held alone sold the wrong joker on a saved Leaf state (2.3% where Swashbuckler's sale wins 57%) |
| Blue Seal: drawn with the hand plus 3 cards an action, each card independently (in the projection the round of the board scored: its jokers' hand size, hands and discards, `long_spec_for`, plus a `Gain`'s, against your projected board's, `LongRun::round`; this round's for skip-or-play); a round's planets the expected number drawn that fit your consumable slots (E[min(drawn, slots)], card.lua `get_end_of_round_effect`): in the projection your slots by Ante 8 (`LongRun::consumable_slots`: today's, less the one each Negative consumable you hold adds, which goes with the card; plus a voucher's, `Gain::consumable_slots`) less the consumables held at round end (below), the ones free now for this round's end (skip-or-play: the money cards it counts as used don't hold one); each deck's own Blue Seals at its own size; Mail-In rank 4 in 13 | `advise.rs` `seal_round_chance`, `seal_planets_a_round`, Mail-In income; `value.rs` `seal_planets_in` | how often a card shows up | game source + logged rounds |
| Consumables held at round end, besides the planets Blue Seals make there: 1 in 4 rounds (none or one, mixed in that share), the same count whatever your slots | `value.rs` `HELD_AT_ROUND_END` | slots the planets can't have (card.lua `get_end_of_round_effect`: a Blue Seal's planet needs the consumables held to be fewer than the slots) | **measured** in the calibration log (2026-10-01): the consumables in each round's first shop, less planets new since the last shop, on the rounds that had Blue Seal cards (one run, 99XSP7C7, rounds 10–13, 2 slots: one held in 1 round, none in 3; over all 13 rounds 7, mostly a Cryptid held before there were Blue Seals). Few rounds, and the logged player followed advice that never charged for holding: log the deck's Blue Seals and the slots with each shop to remeasure, per slot count |
| Glass cards: about 1.5 scores an ante before breaking; what a deck's extra Glass cards add (against the deck with them destroyed, as breaking does) counts for the share of the antes left they last, the rest of the change in full | `advise.rs` `glass_presence`, `value.rs` `without_new_glass` | Glass breaking | — |
| A Standard pack card with a consumable you hold on it: for every held one that goes on cards (`engine::consumable`), one race (`target_race`) over a hand you'd hold with the card in it (the first hand its own value is sampled on, one card swapped for it); credited only when the best targets include the card, nothing the race can't tell apart from them leaves it out, and the best set with the card is clearly better (paired, 95%) than the best set without it (by paired estimate), as the card × (with it used on the card) / (with it used on the best set without it), paired on the same rounds; labelled "assuming you draw it while you still hold" it | `advise.rs` standard pack cards, `target_race` | you'll draw it and keep the consumable for it | the chance to draw it before you'd use the consumable elsewhere (`seal_round_chance`) |
| Deck changes (tarots, pack cards): 300 rounds against your deck (split between outcomes for random effects; 100 for a tarot no option takes as its own value: `TarotBudget`); a board without a chips joker (`CHIP_JOKERS` list) gets a +60 Chips find | `value.rs` `deck_stats`, `TAROT_ROUNDS` | deck changes are small; a chips gap closes by Ante 8 | — |
| Vouchers: Overstock half a reroll a shop, Reroll Surplus $2 a shop, Clearance 25% of ~$8, Hone one buy an ante, a voucher shows 1 in 16, Planet Merchant 1 in 12 on your main hand; one that changes your consumable slots (Crystal Ball) or what a round starts with (Grabber, Wasteful, Paint Brush: hands, discards, hand size) by what it changes in the run (`plan::apply_voucher`, card.lua `Card:apply_to_run`), through `Gain::consumable_slots` / `hand_size` / `hands` / `discards` with its price (`voucher_gain`; a slot's only by its Blue Seal planets: room to hold more tarots and planets isn't modelled, and says so; a hand's money left at cash-out isn't, and says so); this round's odds with it from the same change, the blind's rules on top (`Board::at_blind_set`: The Needle, The Water) | `advise.rs` economy and voucher values | what each voucher saves | logged shops |
| Tags: Meteor's main-hand planet 5 in 12, best 1 of the pick 2; Investment not discounted for arriving later; a Mega Buffoon pack's second pick $2.50 (shop and tag); Emperor average². Any pack can be skipped once while a pick is left: a one-pick pack is worth at least the skip, a Mega pack's second pick at least the skip (`pack_value`); a skipped blind's growth (Throwback) counted by Ante 8 and for this ante's boss and the next ante's (the skip side's board after `Board::after`), the jokers the shops ahead may show measured on your board as it is (their lift on top understated); a tag paying $ per hand played or per discard left unused this run (Handy, Garbage: the tag's config rate) as money now, from the save's counts (`GAME.hands_played`, `GAME.unused_discards`, paid when the tag is given: tag.lua `Tag:apply_to_run`) | `advise.rs` skip-or-play, packs, tarots, `pack_value` | — | logged tags and packs |
| Skip-or-play "close": within 3% | `advise.rs` skip-or-play | noise | (should become `compare.rs`) |
| Shop ranking: ties within 3% (shown as ties: `ShopOption::tied`, ≈ in the page; the band starts at the top's value, so two options just across a band edge aren't shown tied; within one, keeping the money first: leaving, skipping a pack); under 20% this round, score reach decides (no ties shown); a win chance told from none at 3 wins in its rounds (`three_wins`: with none, up to 3 in n can't be ruled out, the rule of three; with 1 or 2 more): next-ante survival counted at least that (its rounds: a third of the simulations, at least 20, `spec_rounds`; a hopeless next ante is then the same factor for every option, and a lucky round doesn't zero the others; a cut at 1% for every option did, shop_gros_michel_for_mail_in 2026-10-05), and a joker the shops or packs ahead may show adds to a survival or expected-buy estimate only from 3 wins in its own rounds (`measured_p`: 100 at the next ante, 20 for a screened joker) | `advise.rs` `rank_options`, `TROUBLE` | noise and hopeless rounds | (should become `compare.rs`) |
| A joker that saves a lost round in the shop ranking (Mr. Bones): on the board each option leaves, the analysis's simulation count of chained runs (`sim::chained_through`: each save played on without the copy that made it) through every blind still to come this ante (a round in progress first), then the next ante's Small and Big Blind at their plain sizes (×1, ×1.5 its base) and its plain boss; each round with what the joker sold or bought adds to it; of the runs that lose a round on score, the share the saves get through, this ante and the next ante among the runs alive after this one (one group of runs each, nothing told from fewer than 3 lost), applied to the option's own losses (`p + (1 − p)·share`, on its survivals with the money walk's finds, and on this round's chance for the trouble line: `ShopOption::p_with_save`; labelled heuristic in the note; measured on the shared runs, inside the ranking's 3% tie band like its other terms, gap 7); the board an option leaves read from its kind (yours, a sale's, a joker bought's: a tarot that makes or destroys jokers, Ankh, Hex, Judgement, keeps yours, as in gap 17's walk) | `advise.rs` `save_lift`, `SAVE_DRAWS` | a save spent once on the rounds ahead, beside what money finds | the save played on in the shop judge (`tests/shop_playon.rs` counts score only) |
| Joker pool: every joker the shops could offer screened on 60 rounds (`screen_sims`) and 100 typical hands (`SCREEN_DEALS`); the best 12 (`rescue_top`) simulated on the full count, best by `rank_value`: win chance gained in this ante's hardest round ×10, plus the best change in a later round's reach (lost or gained; at most +1) ×5, plus the typical hand's gain (at most +5) ×0.1. A later round's reach lost counts in full. With no slot free, the joker it replaces: the next row | `advise.rs` `Options`, `rank_value` | the pool's simulation budget, and a ranking that weighs this ante against the next | the shop ranking's measure (`rank_options`: survive × next-ante survival × long run), as the joker it replaces is chosen |
| The joker a buy replaces (no slot free): every sale and place raced (`compare::race`, outside trouble a round lost outright counted) from a first batch of 32 rounds (`SWAP_FIRST`) to the full count (300), at most 6 swaps after 32 rounds, 4 after 64, 3 after 128 (`SWAP_BUDGET`; halving from all of them where that keeps more), still in at the cap reported as ties; a sale's places that play the first batch the same are one; the next ante's boss and the strength there on draws of their own (`NEXT_DRAWS`, `STRENGTH_DRAWS`: the measure multiplies them, as `rank_options`), a loss at the next boss counted at the three-wins floor on each round (`p + (1 − p)·floor` on the mean, a little above `rank_options`' `max(p, floor)`), the strength under the square root on each round (a little below the root of the mean); in trouble (no swap gets through this round 20% of the first batch, Mr. Bones' save counted: `TROUBLE`, the shop ranking's own line), the score reached decides, the measure second, no ties shown; a save spent on this round leaves one for the next ante's boss only with a second copy (`Board::round_saves`), in the race and in `swap_measure`; the screen's removal measure the same in expectation (`swap_measure`; in trouble when no removal leaves 20%). Not counted: the survival money left adds (the sales differ by a sell value, counted in the long run), money past the planet-level cap (`Spending` is built after the pool: a money joker's income buys nothing there, for a buy as for a sale) | `advise.rs` `evaluate_candidate`, `shop_measure`, `SwapRound`, `swap_measure`, `SWAP_FIRST`, `SWAP_BUDGET`, `NEXT_DRAWS`, `STRENGTH_DRAWS`, `TROUBLE` | noise and cost | every swap on 1,600 rounds against the pick |
| Perishable gone by Ante 8 if its rounds < 3 × antes left; Gros Michel breaks 1 in 6 a round | `value.rs` `lasts`, `advise.rs` | — | game source (exact) |

## Retire list

Narrow rules still in the code, each to be replaced by its stage's general mechanism. Don't
add to this list's kind; remove from it.

| Rule | Where | General replacement |
|---|---|---|
| Suit-joker map, `kickers_matter` list | `sim.rs` `keep_suit`, `advise.rs` | game facts: move to `data/` or the engine; or work out what to keep by scoring with and without a card |
| Played cards ordered with Glass and Polychrome last (`sim::arrange`), the order the engine's arrangement starts from and falls back to | `sim.rs` `arrange` | the engine's arrangement from the cards in a fixed order (the fallback then matters only where an effect reads a card's place: gap 19) |
| Constellation grows from planets in several places | `value.rs` `add_planets`, `spend_once`; `advise.rs` Meteor tag, `grow_constellation` | a planet used as an event in `engine::RunEvent`, applied by `Board::after` (`xmult_from`), counted through `Gain::planets`; `bought_event` then compares what money buys by measured value, not Mult per dollar |
| A short perishable's price dropped from its value | `advise.rs` (`long_mult = 1 + unlock`) | `LongRun::value` with its price |
| What a joker adds to a round (hand size, hands, discards) as a table (its `add_to_deck` effects, taken back by `remove_from_deck`, which the save's `round_resets` holds once it's yours; the `setting_blind` context is the engine's, `Board::at_blind_set`, set on each fresh round from the board played, after the boss's own rule: card.lua `calculate_joker`, checked 2026-10-19 and 2026-10-27; the jokers that context makes give theirs through this table, none while debuffed), and Turtle Bean's shrink by name | `engine/joker.rs` `Joker::round_mods` (read by `advise.rs` `apply_mods` and `Board::at_blind_set`), `value.rs` `long_spec_for` | the center's config in `data/` (`h_size`, `d_size`, `extra`) as game facts; a fading joker's state projected as growth is (`value::Growths`, `grow_antes`) |
| A fresh round's blind beside `fresh` (`RoundStart::blind`, `disabled`: ignored on a round in progress, whose board has its blind), named again by `Spec::blind_key`; and `RoundStart::hands_on`, two of the three numbers `Board::at_blind_set` sets, next to the hand size before it's set (`value.rs` `lr.round` once paired them) | `sim.rs` `RoundStart`; `advise.rs` `Spec`, `fresh_round`, `boss_reroll_option` | a start that is either fresh, with its blind, or in progress (an enum), the spec reading the blind from it; callers read what a round is played with off `at_blind_set`'s start |
| Chaos the Clown's free reroll a shop found by its key (`j_chaos`) | `advise.rs` `money_value_with`, `tests/shop_playon.rs` | a joker game fact for the shop's free rerolls in the engine or `data/` (state_events.lua `new_round`: `free_rerolls` = how many are held) |

## What strong play does

Lines experienced players know win, each with what the simulation needs to find it on its own,
where that stands, and the check that would show it. They aren't rules (nothing in the code
branches on them): a line blocked by several gaps moves those gaps up, and a line with no gap
behind it is a blind spot nobody has written down yet.

- **Small hands over and over to grow jokers that grow when hands are played** (Ride the Bus,
  Square Joker, Runner, Green Joker: Pair is the hand you can always make). Needs a joker's
  state at the round's end in the round's value (since 2026-10-14: Best play plays for it,
  e.g. a 4-card hand that doesn't win yet with Square, a lone card with Green Joker while the
  round stays safe; and the simulated player at its finish, in its look-ahead and, since
  2026-10-17, in a play that doesn't win when it costs no pace), and the player's digs playing
  for it, with growth measured from what it plays (the projection's, since 2026-10-17, from
  rounds played to the round's goals). Blocked: gap 18 (the policy's digs and junk hands, a play
  that gives up pace for growth, a joker replaced by Ante 8 worth nothing before then). Check: on `player_values`' growers board the player
  plays more hands with no face card scored than on the same board with Ride the Bus taken off.
- **Retriggered ×Mult on one card** (Photograph + Hanging Chad, Idol + Sock and Buskin, Baron +
  Mime on Steel Kings, a Red Seal Glass or Polychrome card). Scoring is exact: each trigger
  applies the card's ×Mult and the jokers that read it again (`engine/score.rs`, `PlayRepetition`
  and `HeldRepetition`), tested one joker at a time (`engine/tests.rs`), not the pairs. Building
  toward it is blocked: two jokers on screen are never bought together (gap 23), a new joker
  isn't placed in the best order (gap 26), a partner you'd find later is worth only a
  generic stand-in (gap 27). Not measured: whether a consumable's target search
  puts the Red Seal or the edition on the card that leads. Check: an engine test per pair
  (Photograph + Hanging Chad on a lone King is ×8), and the shop on a board with one of the
  pair owned ranks the other first.
- **Glass and Steel against the last bosses** (their ×Mult doesn't care which hand you can
  make). Needs the bosses ahead in the rounds a deck change is valued on. Blind spot: the
  projection plays rounds with no boss rule and no target (`value.rs`, `long_spec`);
  gaps 12 and 13 cover the target and the horizon, not the bosses. Check: Justice on a card is
  worth more to a deck facing The Eye or The Psychic than to one facing plain rounds.

Endless play (past Ante 8) is out of scope (the question above is winning the run); it rewards
the same retriggered ×Mult.

## Known gaps

**Where things stand** (kept current with every commit that changes it):
- **Now: the simulated player (gap 6),** in steps, each judged by the player benchmark and the
  oracle (D9) and the history check (D12). 1. The last hand digs while a discard is left (done
  2026-10-10). 2. Dig plans for Two Pair from a Pair and a Full House from Three of a Kind (done
  2026-10-10). 3. Pace by playing on: each later hand the best play again on the board the one
  before leaves, the game's own step but The Eye's ban (done 2026-10-11; the Goad fixture keeps its Bonus Gold 3).
  What's left of gap 6: later hands taken as the cards held again (behind the over-counts under
  The Eye, Card Sharp and Ride the Bus with 1 discard; dealing them from the draw pile instead
  was tried and rejected 2026-10-11, below: the pace must judge the hand you hold, so its later
  hands must depend on it), the Goad-like boards' gap of 14 with 3 discards, growth that depends on
  which cards are played (gap 8), each chase branch's own growth (made, failed, stopped), the high one-hand gaps
  (growers, discards: about 20), a free junk hand never banked before the hand that wins (gap 34, a
  fixture's expectation waits on it).
- **Done 2026-10-11:** the search budget's first cut (gap 1): the budget halves many moves
  instead of cutting them to 48 on 32 rounds, and nothing is shown equal on rounds all worth
  nothing.
- **Done 2026-10-12:** late-ante speed, part 1: a simulated round about 40% cheaper (a scoring
  pass skips the contexts no joker on the board answers; playing on's later hands search only
  the plays a full search left unbeaten in chips and Mult, `later_best`); an Ante 7 shop
  analysis 112 → 60 s, an Ante 8 quick pass 2.5 → 1.8 s, every benchmark win rate, oracle gap,
  snapshot and history pick unchanged. The logged 20-60 s quick passes were mostly a busy
  machine (the same states take 1.5-5 s alone) and 10-12-card hands. Part 2 so far: the
  shortlist starts from the decision's own search (a round 16% cheaper again). Measured and
  dropped: work handed out item by item to the threads (they were already busy), fewer
  simulations (300 → 100 saves 13%: the long-run values have budgets of their own), a shop
  quick pass first (still ~30 s: the joker pool and By Ante 8 come before it), an exact cache
  across analyses (money is part of every board: nothing repeats once it changes). Then the
  tarots no option takes as its own value screened (`TarotBudget`): ~20% of a shop.
- **Done 2026-10-14:** playing for a joker's growth, part 1 (gap 18): a round's value counts
  what its hands and discards leave the jokers a round changes, so Best play plays a 4-card
  hand for Square, a lone card for Green Joker or to spare Obelisk while the round stays won;
  49 of 284 saved picks moved, 35 BETTER, 8 tie, 6 unclear, none worse (the judge shares the
  valuation); quick passes no slower.
- **Done 2026-10-14:** playing for growth, part 2 (gap 18): the simulated player finishes with
  the winning play worth most and looks ahead at a growing play and at keeping the cards that
  pay at round end, paired; `player_values` (boards with fixed round goals) measures it: the
  round's value up on 9 of 10 rounds, win rates up on the seal boards, a round 3-13% slower. A
  fixture's expectation now waits on gap 34 (D14, `pending_expect`).
- **Done 2026-10-15:** The Hook in the estimates (gap 15): the engine's `score` runs its
  discard; an estimate averages the pairs it could take, "surely wins" reads the costliest.
  Three Hook boards in `tests/player.rs`: win rates up on held cards (+8, +12 points with 1
  discard or 1 hand), the gap to the oracle 85.0 → 80.5 over the nine, two boards lower (gap 15);
  every other board the same; 3 of 341 saved picks moved (1 BETTER, 2 tie); a round on a Hook
  board whose jokers read held cards costs about twice as much.
- **Done 2026-10-16:** The Hook's two lower boards (gap 15): a junk hand played to dig loses
  held cards to the Hook, which the chase now counts (growers_hook played twice as many junk
  hands); a held card is measured against a card drawn from the pile in its place, over the
  sets the Hook could take. Over 20,000 rounds against the player
  before the Hook: held_hook +3.5/+12.0/+13.5 points, enhanced_hook +0.3 to +1.5, growers_hook
  −0.5/−0.4 (gap 8: its discards' Green Joker Mult); held_hook 4h3d's drop was noise. Every other
  board the same; 3 of 367 saved picks moved (2 tie, 1 unclear). Tried and not kept: the same
  drawn-card measure without the Hook (gap 15): the held boards lost 2.8-8.0 points, as a keep
  is all or nothing and a card that only guards Raised Fist's read blocked the dig.
- **Done 2026-10-16:** a dig plan scored in the hand it's completed in (gap
  15): when the score reads held cards, its completion and the hand its dig leaves hold the
  dig's leftovers and cards drawn; cards held back for their held value take dig slots on every
  board. Over 20,000 rounds: held +4.2/+4.6/+2.6 points, held_hook +16.9/+5.9/+10.5; two scratch
  boards lower (Raised Fist 4h1d −1.7, Blackboard 1h3d −0.8: the chase's optimism, gap 6). The
  drawn-card held measure on every board and a keep weighed against the dig were measured and
  rejected.
- **Done 2026-10-17:** the projection's growth in rounds measured (gap 18): the jokers a
  round changes grow or fade by Ante 8 as 16 runs of the simulated player's rounds leave them
  (`value::Growths`), not by a table; a joker the runs use up doesn't last. No fixture's Best
  play moved; 6 of 136 saved picks moved (1 BETTER, 4 tie, 1
  unclear within noise; the judge shares the valuation); a shop analysis 0.1-0.4 s slower, a
  quick pass ~0.1 s. Pool jokers that grow now carry their measured growth (Obelisk on a run
  whose most played hand is High Card ×1.10 → ×3.34 by Ante 8; Wee and Runner lower, Square
  and Castle higher than the table). Ride the Bus can now be replaced (below).
- **Done 2026-10-17:** the simulated player's plays that don't win weigh growth (gap 18,
  `spare_instead`: a play that grows the jokers more, when it costs no pace, on 16 paired
  futures), and your jokers' growth measured again on runs played to the round's goals (the
  second pass, `played_for`). `player_values`: Spare Trousers +0.012/+0.018 of a round (win
  +1.1/+1.7 points), growers +0.001, nothing worse; the benchmark and the oracle gap unchanged
  (no goals there); 2 of 136 saved picks moved, both Ride the Bus states now playing a junk hand
  with no face card instead of discarding (ties, +0.4%); no fixture's Best play moved. A quick
  pass on the saved states with jokers a round changes: median +14% (the second pass 0.07-1.7 s,
  most at late antes), the fixture's 2.4 s as before. Ride the Bus on `ride_the_bus_queens_dig` stays at +1.1 by Ante 8 as played for,
  replaced: on that flush board its resets are the flushes' price.
- **Done 2026-10-17:** Best play's round values what it leaves in this ante's blinds still
  ahead (gap 18/28, `value::Ahead`): the Big Blind and the boss played one after another from
  the board it leaves, for the jokers' numbers (a step up, a step down, a fall to none; the
  share multiplied with the Ante 8 worth), a Blue Seal's planet, a dollar and a card worth
  drawing, each share counted where the paired runs show it.
  `ride_the_bus_queens_dig`'s expectation restored (the Queens kept out of the dig); 6 of 136
  saved picks moved (2 BETTER, 2 tie, 2 unclear within noise); a quick pass 0-6% more CPU,
  21-44% where a grower is now played for.
- **Done 2026-10-18:** what a round leaves in this ante's blinds still ahead combined in the
  odds (gap 18): each joker's, planet's and dollar's change a shift of the log-odds, placed at
  one chance, your board's with what the shops before the boss add (as the shop ranking counts
  them; the quick pass as before), the shifts added and the Ante 8 factors multiplied, so
  several resets no longer floor a won round at nothing and growth never passes a sure pass
  (measured against the joint runs on 43 saved states). One fixture's pick moved
  (`ride_the_bus_queens_dig`, still keeping the Queens, tied with the old pick); 3 of 139 saved
  picks moved (1 BETTER, 2 tie); no measurable cost.
- **Done 2026-10-18:** every change in the blinds ahead measured on a board carrying the shops'
  find (gap 18): every score ×x, sized shop by shop on the boss alone to the shop ranking's
  estimate, then to at least 10% on the chain, so no change is moved from one chance to another
  (on 35 lifted saved states the transport was off by 0.084 of a round, the find's shape
  0.039). One fixture's pick moved (`ride_the_bus_queens_dig` plays a Queen again: its chain is
  97% with the find; the owner's line waits on The Pillar's rule, gap 15); 3 of 139 saved picks
  moved (1 BETTER, 1 tie, 1 unclear +0.30% ± 0.41%), all quick passes on boards below 10%;
  CPU within noise on 8 in-blind states (a quick pass −8% to +8%, a full pass −4% to +3%).
- **Done 2026-10-18:** the money in the blinds ahead measured as the other changes are (gap
  18): your board carrying the shops' find for $10 more against the find, each blind's
  multiplier raised from where the find landed by what $10 more adds to the shops' lift (with
  one blind ahead the chance that gives, with two the runs, read off the points the find's own
  search tried), so it's no longer read off the estimate's slope at another chance: on 35
  saved states off a 1,200-run find by 0.021 of a round per $10, against 0.044 before (0.076
  against 0.20 below 30%, near the floor; with two blinds ahead about the same, 0.018 against
  0.019). No fixture's pick moved; the 139 saved picks the
  same (quick passes: no shops counted there); CPU within noise (with one blind ahead no runs,
  with two one set more).
- **Done 2026-10-22:** the counters a round leaves the jokers counted in the blinds ahead (gap
  18): Loyalty Card's place in its cycle, Yorick's discards, Seltzer's hands left, each value
  the round can leave a shift of its own, so the hands a round takes land Loyalty Card's ×4 in
  the boss or off it (on a marginal test board 0.41 against 0.09; the saved Loyalty states all
  get through their boss whatever the phase). No fixture's Best play moved (none holds a
  counter); 0 of 136 saved picks moved (14 of them from runs with Loyalty Card or Seltzer); a
  quick pass on saved Loyalty states 4-5 sets of runs more (Best play's goals +0.1-0.6 s).
  Left in gap 18: Yorick's count by Ante 8, the reference at a counter no won round leaves, the
  simulated player's look-ahead one hand deep for a counter.
- **Done 2026-10-22:** the simulated player's sparing play judged by the round played on
  after it (gap 18, `line_pace`: the play once, then the policy's best play, as the pace plays
  every decision, not the same cards every hand), and the finish's look-ahead to growth counting
  what winning now takes off the jokers (a reset playing on spares). `player_values` Spare
  Trousers +0.0031/+0.0035 of a round, the rest unchanged; no fixture's Best play moved, 0 of
  136 saved picks moved; a quick pass as before but `ride_the_bus_queens_dig` +17-20%. Replaces
  PR #10 (measured, not kept: gap 18). `psychic_kings_zero_score` stays pending: its moves are all within the tie
  margin, and 7 of its 11-12 resets now come where the round is behind pace on the policy's
  play too (the target test, a cost budget).
- **Done 2026-10-24:** over-buying in the shop measured on the owner's Gold Stake runs that
  ran low, against leaving by playing on (gap 17, `tests/shop_playon.rs`): not there. Joker
  buys and Buffoon packs ranked first were right (+0.18 and +0.04 of the bosses' joint chance),
  the top rerolls a wash that costs $3. Gap 17's fix tried three ways and not kept: the money's
  finds lean high (the best of every draw, read off the screen's chances), so they come first.
  The bot buys only what clears keeping the money by the ranking's noise and sells by place.
  (Superseded 2026-10-26: the finds' lean wasn't the screen's but how the money was spent, gap 17.)
- **Done 2026-10-24:** the joker a buy replaces chosen by the shop ranking's measure (gap 36):
  every sale and place raced on the same rounds (`shop_measure`, one function for the shop and
  the race; Mr. Bones' save counted in the race), ties reported, the by-name sell rules and the
  second long-run pick retired, a Negative joker never sold to make room (gap 33), the
  projection's own swap and the floor judged with the money. On 32 saved full-slot shops (the
  owner's runs with money jokers, Cartomancer or growers) 167 of 398 sales moved (43 with the old
  sale among the new ties); whatif on 2,000 rounds over 92 of them: 26 old sales were of a
  Negative joker (not allowed), 21 better on this ante's and the next boss, 38 the same, 7 lower
  there and traded for the long run (3 with the old sale tied). Fixture
  `shop_golden_kept_by_name` (Golden kept by name cost 20 points at The Eye). 2 of 135 saved
  Best play picks moved (1 tie, 1 unclear within noise): the long run's values moved under them.
  A full 8-slot shop as before (51 s); most saved shops 10-40% slower, a few up to 80%, one Ante 7 shop 2.2×
  (The Hook with Raised Fist kept, the joker the measure keeps). Left: gap 36 (money past the
  cap, Mr. Bones' save in the shop ranking).
- **Done 2026-10-25:** a joker's money is the round's as the game pays it (gap 36): measured on
  the same rounds of this ante's plain blinds with and without it (scoring, discards, the
  engine's round end with every `calculate_dollar_bonus`, hands left), carried as income; the
  income table (`income_per_ante`) retired. Left: gap 36 (money past the cap, this ante's rate,
  Matador).
- **Done 2026-10-25:** a joker can be let go for money, in the projection and as a shop option,
  by the same measure (gap 33): the projection's keep-or-let-go (`Fate`) tries selling outright
  too (no find, its slot left empty; a Negative's only way, its slot gone with it), a let-go
  joker's sale counted once (less a typical find's price for a find, `find_price`), and the find
  a replaced joker leaves is now held in the slot it was chosen in (it went back to the joker's
  own place: on `shop_next_ante_hopeless` Mail-In's find leftmost cost l0 9%); in the shop,
  selling each sellable joker is an option (`sold_now`: its rounds without it, its sale, a
  round's rent and money where the projection lets it go anyway), ranked like every option,
  leaving first on a tie (D8). A Negative's slot also goes when it runs out or is used up, and
  comes with a Negative bought; what a sale does that isn't modelled is said (Diet Cola,
  Invisible Joker, Campfire). The owner's 1,131 saved shops with a rental: whatif on 2,000
  rounds kept every sampled rental as right this ante (selling Misprint 98% → 2% at a Big Blind,
  Swashbuckler 90% → 85% at the next boss; fixture `shop_negative_rental_swashbuckler_kept`).
  On the fixtures Best play moved on none; sales rank above leaving on three (a perished Half
  Joker; Faceless Joker and Mail-In Rebate, let go anyway, $2 lifting the next ante's survival:
  gap 17's finds); orders moved on two (`burglar_banner_no_discards`: a band edge, Baseball
  Card's long run −5%; `shop_golden_kept_by_name`: Grabber ×1.46 → ×1.40, tied with the reroll
  now, the find's price keeping Superposition, Swashbuckler and Droll on the projected board,
  before it a $2 margin let Wrathful Joker go on another fixture and moved Business Card 11%: the
  line is exact); 2 of 135 saved
  Best play picks moved (a tie; −1.07% ± 0.55% unclear on a face-down discard at The House, from
  the replaced jokers' net money in the round goals). Left: ties by paired differences (gap 7),
  only this ante's hardest round read, the keep-or-let-go judged one joker at a time and on an
  exact line.
- **Done 2026-10-27:** The Pillar and Verdant Leaf in the simulated round (gap 15): one port of
  `Blind:debuff_card` in the engine, The Pillar's cards played this ante read from the save and
  carried from round to round, Verdant Leaf's sale made by the simulated player (and before Best
  play's moves). 38 of 103 saved boss chances before The Pillar moved by more than 0.02; parity's
  blind setups 532 → 548 of 576; 2 of 135 saved picks moved (1 better, 1 tie). Left: Best play
  weighing its own cards' cost at The Pillar and racing the sale, the boss row chaining the blinds
  before it.
- **Done 2026-10-27:** the money's finds made honest and counted from the board each option
  leaves (gap 17): the money spent shop by shop as a player buys (no hindsight across shops, no
  income spent before it's earned, a debt paid back), a find in a free slot or in place of the
  joker whose loss costs least (yours sold for it, or the one the option buys), the jokers kept
  as score gains added, the walk starting from the option's own chance, scores and slots;
  leaving walks out, a reroll is its money's chain. Against the same money played on, on 25
  saved shops (`money_finds`): this boss 0.036 → 0.018 high, the next ante's boss 0.062 low →
  0.021 high (mean absolute error 0.044 → 0.023, 0.128 → 0.046); the screen's winner's curse
  wasn't there on average. 45 of the 144 Gold Stake shops' top picks moved: 28 judged by playing
  on, +0.11 of the two bosses' joint chance in all (1 clearly better, none clearly worse), 17
  unjudged (tarot packs). Five other versions measured and not kept (gap 17). Left: gap 17.
- **Done 2026-10-27:** the round's start (`Board::at_blind_set`): the jokers' `setting_blind`
  context as the game runs it, left to right (Chicot disabling a boss after the rest, Riff-raff's
  Common jokers drawn per simulated round from the one joker pool, `in_joker_pool`, Marble Joker's
  Stone card carried to later rounds), the Juggle Tags you hold on the next round played (skip-or-
  play's Big Blind skip too), what a joker gives while held none while debuffed. Snapshot: only
  pool candidates now simulated (Riff-raff, Marble) and what reads the pool moved, no top option
  or Best play pick; 0 of 135 saved picks moved; the player benchmark the same on every board;
  parity's blind setups 548 → 549 of 576, nothing new. Ceremonial Dagger and Madness were tried
  and left not modelled: a joker the advice adds goes at the right end (gap 26), where a
  rightmost Dagger ate every buy, and a joker's loss isn't priced (gap 8); both first.
- **Done 2026-10-27:** Mr. Bones' save in the shop ranking (gap 36 (c)): a save is spent once
  for each copy (`Board::saves_lost_round`, the saver dissolves in the simulated round), and
  every option's survival counts the share of lost rounds the saves on its board rescue through
  this ante's blinds and the next ante (`save_lift`, chained runs); the trouble line counts it.
  On the fixtures only `shop_bones_save_counted` moved (Mr. Bones for Gros Michel 7th → 4th,
  0.298 → 0.446, just below leaving's 0.461: this ante 0.60 → 0.84 against leaving's 0.76, the
  next ante 0.57 against 0.61 without Gros Michel); the 135 saved Best play picks and parity
  unchanged; the player bench not run (no bench board holds a saver). Money past the cap
  (gap 36 (a)) tried and parked: the bias it exposed (×0.78 a buy) was read as the policy's
  digs, but the whatif row behind it was a rental's rent (2026-10-28): its cause is open.
- **Done 2026-10-28:** the policy's digs count what their discard does to the round's later
  hands (`dig_board`, `throw_order`, gap 8): win rates up on the Hit the Road and Castle
  boards (most by 5-8 points), the oracle's gap there narrowed; weighing digs by the round's goals measured
  as not paying (the oracle's gap beyond winning ≤ 0.005 of a round).
- **Next:** a free hand banked before the
  hand that wins (gap 34); Ceremonial Dagger's value (gap 8); the blind odds shown and the
  outlook chained as Best play's blinds ahead are (gap 28).
- **Not built yet** (ideas, not gaps): a rush check ("skip every small and big blind to Ante
  8", with perishables that age only on played rounds, separate from the options ranking); a
  chance to beat the next boss after the shops before it (a separate forecast line, never part
  of the win chance); interest in the
  long-run money value (dropping under the interest line; measured 2026-10-24, gap 17: not
  where any over-spending was); Director's Cut's later boss rerolls.

Ranked by how likely each is to cause the next round of patch-on-patch.

1. **Best play's search still misses sometimes.** Measured by `search_against_reference`: the
   budget cuts moves on few rounds (losing is rare, so early rounds rank moves on money). With
   the dig plans in the simulated player (2026-10-04) every pick passes (wall_cryptid seed 44,
   1.64% ± 0.51% below the best before, the Cryptid-first Flush cut at 64 rounds, now picks a line
   1.5% ± 0.6% above the reference's best), but nothing in the budget changed, so the risk stays: with chases worked out dig by dig (2026-10-05) wall_cryptid seed 42 picks a plain Two Pair 1.01% ± 0.47% below the reference's best, the Cryptid-first Flush (passes, barely).
   How each move left the race is now recorded (`compare::Race::left`, printed by
   `search_against_reference`), 2026-10-05: on wall_cryptid seed 44 the Cryptid-first Flush,
   0.14% below the reference's best, was called clearly worse after 32 rounds (a steady small
   money edge, no spread, before a rare lost round could show). "Clearly worse" now counts a
   round in the move's favour not yet seen (rule of three, where a round can be lost
   outright: Best play); with it no move near the best is called worse early, and every pick
   still passes (seeds 42–44 every fixture, 45–46 wall_cryptid; seed 42 0.98% ± 0.44% below the
   best, seed 44 0.31% ± 0.54%, seed 46 0.53% ± 0.49%, was on the best). The budget's cut
   then ranked by value on few rounds, with the same blind spot; it now ranks by the same bound
   as "clearly worse" (`compare::better_by`: each move's paired difference to the leader, the
   unseen round counted, less 2 standard errors), 2026-10-05: wall_cryptid picks the
   reference's best on seeds 42, 43 and 46 and is 1.28% ± 0.49% above it on 44; every other
   pick passes as before (seeds 42–44, every fixture). What's left: where far more moves are
   within 1% than the budget keeps (big_blind_keep_spade_ace: 77–108 of 436), which near-tie
   is picked is still noise (seed 42 now throws the A♠ the owner objected to, 0.50% ± 0.30%
   below the best, as close as the line it picked before); the cut is a cost limit, and a
   larger first batch is the lever if that matters. It does matter where the leaders are far
   apart from the rest but noisy on 32 rounds: the first cut threw out the best move on
   boss_hook_ante1 seed 44 (2026-10-09, 5.1% below the best) and on big_blind_crafty_close_race
   seed 42 (2026-10-10, a history state the last-hand change moved: its two best moves, 0.33 and
   0.32, cut "budget after 32 rounds", the pick 31.9% ± 4.2% below; seeds 43 and 44 pick the best).
   Why (2026-10-11): on crafty, 355 moves still in after 32 rounds had about 54 distinct
   results between them (a round's value is its win, spare hands and money, so different moves
   can match round for round), so the 48 kept held 17 distinct results and the best sat 57th;
   moves with the same results differ in the reference (0.33 down to 0.17), so one per group
   can't stand for the rest. On the Hook state the results were distinct, but 32 rounds barely
   ranked them (the cut's order against the reference: correlation 0.27; the best three 78th,
   117th and 222nd). Since 2026-10-11 the budget halves many moves (`compare::halving`:
   successive halving from a first batch of 16 rounds, 436 → 218 → 109 → …, never below the
   table): every fixture in a blind on seeds 42–44, 2 misses → 0, the mean gap to the
   reference's best 1.40% → 0.14%, 7 picks moved (6 closer, keep_spade_ace seed 44 −0.32% →
   +0.17% among 163 moves within 1%). Halving from 32 rounds picked the same but made the quick
   pass 34% slower (Antes 5–7 40–75%); from 16 rounds −4%. The history check then found a pick
   80% worse in a round won 3% of the time (fixture `hopeless_glass_junk_play`): the first 16
   rounds were lost by every move (the same draws), and `paired` showed them all equal on it
   (3 × the largest round value, 0, within 1% of the leader's, 0), so the race ended on no
   evidence; this was there before (at 32 rounds, less often). Nothing is now shown equal on
   rounds all worth nothing. The rounds raced on 101 of the owner's states rose 13% (Antes 4–6
   20–31%, mostly near-hopeless rounds that had stopped on that false tie). History check: 15 of
   149 picks moved, the crafty state BETTER (+31.9% ± 8.1%), 5 ties, 9 unclear (at 1,600–6,400
   rounds within 1%, or noise in a round won 2% of the time), no WORSE. What's left: which
   near-tie is picked where hundreds are within 1% is noise (keep_spade_ace); the target search
   (`TARGET_BUDGET`) still cuts to a fixed 12 after 16 rounds, however many sets; a round no
   move can win races to the cap (no finding separates moves all worth 0: 2–3× the rounds,
   ranked in the end by how close lost rounds came; racing on that measure while nothing is won
   would stop it early, but would also rank rare-win rounds by it, unmeasured); in a group of
   moves with the same rounds, all won (closeness is 0 then), the cut's boundary falls by the
   cards' order. 2026-10-18 (after Best play's round took the blinds ahead in the odds, gap 18):
   on `ride_the_bus_queens_dig` seed 42 the pick (play 10♦ 8♦ 6♣ 4♦) is 3.3% ± 1.0% below the
   reference's best (discard Q♣ J♥ 6♣ 3♥, cut by the budget after 16 rounds); seeds 43 and 44
   pick it. Both keep the Queens unplayed.
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
   with, valued by `LongRun`, and let the look-ahead weigh more than what `RoundGoals` counts
   (seal planets, money, the jokers a round changes: since 2026-10-14 the look-ahead weighs all
   three, gap 18).
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
6. **The simulated player's policy (`decide_cards`) is still partly a rulebook.** Behind, a
   chase is decided by expected points: the rest of the round with it, worked out dig by dig
   as the policy plays it (`chase`: honest odds, a lesser plan it keeps alive valued where it
   ends the chase, a failed chase valued at the hand it leaves, a junk hand costing a hand),
   against playing on. On pace it's still rules: one card away, breaking the best play's draw,
   and half a best play more than playing on (`ON_PACE_MARGIN`). A search inside every simulated round (the oracle in
   `tests/player.rs`) costs hundreds of times the policy, and the advice runs simulated rounds
   everywhere, so it only measures (its alternatives follow the game's hand rules: suits as
   flushes count them, runs with Four Fingers and Shortcut; a straight flush's suited run
   isn't one of them, so its gap is understated where those decide). Predicted against made
   (every board, 1,000 rounds, 2026-10-05; before the dig-by-dig chase in brackets): straight
   flush 55% / 50% (74% / 60%), Four of a Kind 25% / 18% (58% / 13%), Full House 66% / 64%
   (88% / 66%), straight 87% / 65% (85% a straight or better), flush 94% / 72% (80% or better).
   Known misses: a chase is weighed in points, not the win (what's past the target counts as
   much as what's short of it), yet every cap at the score still needed lost (below); since
   2026-10-10 the last hand digs while a discard is left, so a last hand's chase is weighed
   that way too (where only winning counts: a plan short of the target can beat one that
   reaches it), a last hand whose best play wins on its average but not surely (Lucky cards,
   Misprint) is still played without the free discard (on pace), and "play on" with a win on
   the table (`play_on_instead`) still ends at the last hand whatever discards are left; after a
   dig, made, failed or stopped, a later hand is worth a later hand of playing on the hand you
   hold now (`later`, from `playing_on`): the growth of what you hold, not of the hand played
   then (a Flush made on Spare Trousers is credited with the Two Pairs' growth, a Straight on
   Runner without its own, a failed chase's hand with the growth of cards it threw), and a lesser
   plan ends the chase by the flat pace (its score every hand left); the general fix is each
   branch's own `playing_on`, from its completions; a later dig is decided against the hand the dig leaves, not by the policy's
   own test at its next decision (which sees the new cards, and on pace its margin and rules);
   when the policy doesn't chase, it still discards toward a better best play while behind,
   which is worth more than playing on it's compared with; a junk hand's own
   points aren't counted; flushes and straights are still predicted well above made, since a chase is
   also cut by a better plan after a dig and by hands that aren't plans (a Full House from
   Three of a Kind); a lesser plan's progress across digs isn't counted (only its cards in one
   dig); a plan's score holds the cards worth holding beside it, and when the score reads held
   cards also the rest of the hand then (the dig's leftovers and cards drawn, since 2026-10-16),
   but not as a hand of its own (a pair among the cards drawn); the oracle (200 rounds a board) wins 3.6 points more on average (4.1 before the
   dig-by-dig chase), most with pairs and 3 discards (9.5 ± 2.4), Smeared with 1 (9.5 ± 2.4), held
   cards with 3 (9.5 ± 2.4), faces with 3 (7.0 ± 2.4) and high cards with 3 (7.0 ± 3.8); on the boards added 2026-10-09 (1,000 rounds) most with two Spare Trousers and 3 discards (9.8 ± 1.4, under The Goad 13.3 ± 1.4) and on the goad_digs_gold_three fixture's fresh round (14.4 ± 1.4): pace took every later hand to score like the best play now, which one strong card inflates (there, a Bonus pair of 3s counted as on pace, so Best play threw the Bonus Gold Seal 3); since 2026-10-11 playing on counts what the jokers grow or fade by each hand and the round's memory of it (`playing_on`), and the Two Pair plans and the last hand's dig took most of the rest: kept, the Bonus 3 is 0.69 ± 0.39 points below a plain one there (was 2.5), keeping beats throwing it, and the same cards are still taken to come back every hand; on pace, a
   chase is weighed by score (`ON_PACE_MARGIN`), not by what the round is
   worth (`RoundGoals`: spare hands, discard money), though the round is likely won either way;
   plans cover seven kinds of hand (Two Pair from a Pair and a Full House from Three of a Kind
   since 2026-10-10: the Spare Trousers boards' win rates +13 to +20 points, their oracle gap
   15.0 → 10.0 and 6.5 → 1.5; a plan's odds count only the ranks it keeps, so a pair among the
   cards drawn isn't counted, which is why it keeps at most two singles, `AIM_SINGLES`); playing
   on (2026-10-11; 2,000 rounds a board: mean 61.03% → 61.34%, nothing else moved; the
   benchmark 42.3 s → 46.1 s, a play searched again only when what it scores by moved): Spare
   Trousers with 3 discards +7.6 points (oracle gap 10.0 → 4.0), with 1 +1.3 (1.5 → 0.0), the
   growers with 3 +2.3 (6.0 → 2.5), under The Goad +4.4 and +2.2 (the gap with 3 discards stays
   at 14.0: the oracle gained as much); the growers with 1 discard −1.6 (real at 8,000 rounds):
   a growth that depends on which cards are played or thrown (Ride the Bus resets on a face card,
   Green Joker loses Mult to discards, gap 8) is counted as if the same cards came back each
   hand, more than the round gives. On the boards added 2026-10-11 (2,000 rounds, the half
   points of the player before): Card Sharp, Acrobat and Obelisk +11.4 with 3 discards (the gap
   16.0 → 18.0 ± 3.5: the oracle, which plays its futures with the policy, gained as much),
   −1.6 with 1 (Card Sharp's ×3 for a hand type again counted as if the same cards came back,
   the gap 1.5 → 4.5 ± 2.2); Ice Cream and Seltzer −0.9 with 3 discards (8,000 rounds; the gap
   2.5 → 4.5 ± 2.1: Ice Cream's fade lowers the pace a little and the player digs a little more
   often, not explained further); the one-hand rounds and the rest within 0.2. The same assumption overcounts Vampire (it eats the
   enhancements again every hand) and undercounts Hiker (its chips on the played cards are lost)
   and Lucky Cat: the board steps on with every roll failing, so growth on a roll (Lucky Cat,
   Space Joker's level) isn't counted. The round's
   memory is the game's (The Arm lowers the hand, Card Sharp, Obelisk, Supernova and Acrobat
   follow, The Mouth's lock holds whatever is drawn) but for The Eye's ban on the hands played
   since: with the cards held standing in for the ones drawn, every later hand under The Eye was
   a lesser hand of these cards, and the player dug where it should play (the trousers_eye board: 42.2% and 40.8% with 3
   and 1 discards against 50.1% before the change; without the ban 53.5% and 51.4%, the oracle
   gap 5.0 → 4.0 and 6.0 → 6.5), which in turn counts a growth The Eye would stop (Spare Trousers
   credited with Two Pairs that can't be played again); both are the cards held taken to come
   back. Dealing the later hands from the draw pile instead (4 seeded deals of the pile in an
   order of its own, the cards left after each play and the ones drawn, the board stepping on;
   2,000 rounds a board, 2026-10-11) was rejected: with no discard spent on a later hand the
   17 older boards' mean fell 61.34% → 55.93% (plain with 3 discards −24.2), with the discards
   left spread over the later hands 57.60%, and also asking that playing now beat throwing the
   cards outside the best play first (both on the same deals) 57.37%, at 1.6–2.2× the time.
   Split in two: drawn hands in the on-pace test alone 57.63% (the whole loss: the player spent
   its discards chasing and then dug with junk hands, a High Card junk dig first in 17% of plain
   rounds against 5%), in the chase's `later` alone 61.43% (within noise, at 1.8× the time). The
   reason is general: three of four hands are later ones, and a drawn estimate barely depends on
   the hand held, so the pace stops judging that hand and asks only whether the round is
   winnable on average, a coin flip at targets near the half point; the cards held taken again
   are what make it a test of this hand. A fix for the over-counts must keep that: correct what
   the cards held taken again would repeat that real draws wouldn't (a hand type again for Card
   Sharp and The Eye, a growth's condition), not replace them; a straight two ranks away is dropped when a straight one rank away keeps the same
   cards (an odds judgement the chase now makes itself); face-down cards are planned with as if
   seen. It cost time (a simulated round about 40% longer, mostly the best play of the hand each
   plan's dig leaves, worked out for every plan in every decision), won back by speedups that
   leave every decision identical (2026-10-05; `player_win_rates`, 2,000 rounds a board, the
   average of 3 runs on one 4-core machine: 34.3 s before the chase, 47.8 s with it, 30.5 s
   now; the same per-board results to the last digit; on the owner's machine 34.1 s → 20.8 s,
   and the private fixtures' full analysis 169 s → 107 s, its snapshot identical): the best plays of a decision's hand and
   its parts share hand detection, and floors no held card takes part in (`HandParts`,
   `Outcome::held_used`); a floor that rolls nothing is its average (`Watched`); draw odds are
   cached like flush odds; 5-card plays that can't all score skip detection; no formatted card
   keys in `aims`. What's left is mostly the best play of each decision's own hand (the
   scoring engine, about 40%) and `aims` (about 12%). Already tried and rejected (paired against the version before on 6,000 fresh rounds a
   board, seed 1234, 2026-10-05; the oracle (fixed hand rules) judged the moves where the third
   one differed on Four Fingers + Shortcut and Smeared, 3 discards: the old moves won 101 to 23
   and 120 to 31, −6.6 and −6.4 points a move): the old rule (odds × score × (hands − 1, 1 to
   2) against the best × hands, a failed chase worth 0) with honest odds worked out dig by dig
   (one dig alone never passes, so a one-card-away straight flush the old rule made 99% of the
   time was never chased: −6 to −7 on those boards); the old rule on every dig's odds with
   plans ranked by honest value (−0.04 ± 0.06); its form with the hand a chase ends with valued
   (+0.01 ± 0.08: plain +4.3, Four Fingers + Shortcut −7.5, Smeared −6.2), the failure at the
   best play (−0.30 ± 0.07), later digs weighed against the cards kept (−0.11 ± 0.06); points
   capped at the score still needed, in the old form (−0.34 ± 0.12) and in the expected-value
   form (−0.49 ± 0.11; 4 points lost before, 2026-10-04); expected points with lesser plans on
   their middle completion (a flush draw's can be a straight flush: the straight was crowded out
   of the finalists, straight −0.55 ± 0.17); never chasing on pace (+1.15 against +1.71 with the
   0.5 margin; both margins measured before the hand a chase leaves counted the cards worth
   holding); dropping the on-pace one-card-away rule (+0.08 ± 0.03 more, but held cards −1.9 ±
   0.25; again −1.83 ± 0.24 with the hand a chase leaves counting the cards worth holding); a
   junk hand's own points counted (the best play of the cards it throws, at every junk dig:
   −0.55 ± 0.09, faces with 1 discard −2.7, discards −2.6; Four Fingers + Shortcut +2.4); lesser
   plans found among the plans of the hand a dig leaves rather than the hand's own plans
   (−0.03 ± 0.05: Smeared +1.0, plain −3.9 with 3 discards, and slower). The 10% odds floor and the (hands − 1, 1 to 2) factor are gone: the comparison makes
   them redundant (identical results without the floor). The general fix: weigh a chase by what
   the round is worth (`RoundGoals::value`, the win first), on pace as behind, with the best
   play after a dig estimated rather than kept. A dig's cards (`dig_for`: the up to 5 of fewest
   chips outside the plan and the cards worth holding) break ties by their place in the hand: a
   10, Jack, Queen and King are all 10 chips, so which of them a dig throws is the hand's order,
   not what each is worth held or toward another plan, and since 2026-10-16 that order also moves a
   plan's score through the cards its dig leaves (seen in a review, 2026-10-16; older
   than it). A plan more than one card away is completed by random fills drawn from its fits in
   the pile's order (`aim_completions_held`), so the same cards in another order can score it
   differently (older than that change). Three stand-ins answer "what's held beside a play": playing on
   takes the cards held now to come back every hand (`playing_on`), the cards worth holding are
   measured left out or, under The Hook, against one card drawn (`held_value`), and a dig plan's
   completion holds what its dig leaves and cards drawn (`held_beside`); one function for the
   hand held after some cards change, called by all three, would keep them from disagreeing
   (the same review). Cards that pay at the end of the round while held (Gold, Blue Seal) are set
   aside before a decision (`decide_with`) and held beside none of these scores: under Blackboard
   a red Gold card held doesn't stop its ×3 here, and Raised Fist never reads a Gold 2 (the
   same review; older than it). Since 2026-10-16 a plan's completion holds the cards drawn beside it, so plans on
   Raised Fist and Blackboard boards are no longer undervalued against playing on; the player
   chases more there (Raised Fist with Joker 4h1d: 0.13 junk hands a round played to dig, none
   before; flushes 46% more often) and two scratch boards lost a little (Raised Fist 4h1d −1.7 ±
   0.3, Blackboard 1h3d −0.8 ± 0.3 points): the chase's optimism above, which the undervalued
   plans had offset. The oracle (200 rounds) says the new chases on Raised Fist 4h1d are the wrong
   ones: its gap +4.0 → +7.0 ± 2.6 (the player 49.0 → 45.5%, the oracle level), Blackboard 1h3d
   +4.5 → +5.5 ± 2.8, Raised Fist 4h3d +18.5 → +12.5 ± 3.2; the junk-hand dig the chase takes
   with one discard left is where to look.
7. **Shop ranking and skip-or-play aren't confidence-based.** They tie within 3% instead of
   using `compare.rs`, because their options are valued by separate simulations, not round by
   round on the same draws. Near zero, a chance at the next ante's boss moves by whole simulated rounds
   (4 of 100 against 3 is ×1.33, far past the 3% ties), so count noise can still decide there.
   Measured by seed alone (`BAV_SEED`, seeds 42–46, the 3 shop fixtures, 2026-10-05): the top
   option is the same on every seed in two; in shop_money_tree_rich Money Tree and Arcana Pack
   swap (seeds 45–46), always within 4.5% (their difference varies about 3% between seeds, so
   the 3% band is narrower than the noise there: shown as a tie on some seeds only). What moves
   is next-ante survival, almost all of it the shops ahead (`survival_with`: 0.26–0.32 against
   0.01–0.08 without them), where $81 left falls under a reroll step that $86 clears on some
   seeds. Lower options swap within their ties. Since 2026-10-25 a sale of a joker the
   projection lets go anyway differs from leaving by a round's rent and money only (its rounds
   the same draws from the contribution pass), which the 3% band often can't tell from noise
   (a weak Ante 2 board's levels it clears, a rich one's not): then shown tied, leaving first
   (D8), however often it repeats shop after shop (a perished rental pays $3 a round until
   sold). Ties told by paired differences would separate it. And where this ante's hardest
   round is hopeless whatever you pick, the money's step-shaped estimate decides: on run
   A2LFYPWG's Ante 5 shop at $94 (The Plant 0.7%), a Negative rental Joker's $1 sale lifted the
   next ante's survival with the shops 0.037 → 0.040, ranking its sale level with leaving
   although whatif puts selling it 96% → 84% at the Big Blind (a round the shop measure doesn't
   read: only the hardest). Since 2026-10-26 a sale's score ratio also scales what its money
   finds (gap 17), so what moves that ratio moves such near-ties too: a dead suit joker's sale
   scores 3% under your board at the next ante, as the simulated player's suit map
   (`sim::keep_suit`, retire list) still chases its suit while it's debuffed.
8. **The policy weighs what a discard does to the jokers only in this round's points, and only in
   its digs.** The simulated round plays on the board each discard leaves (`Board::discard`), so
   Best play's moves are valued with it. Since 2026-10-28 the policy's digs are too: after a
   first dig that discards, a plan's chase scores its completion, the lesser plans it keeps
   alive and the hand a failed chase is left with on the board the dig's cards leave, and a
   later hand played on by what that board makes of the best play now (`dig_board`: what the
   engine's step does to the board's state, the jokers' `discard` context and Burnt Joker's
   level; the count of discards left is put back; `ChaseRound::lift`); playing on now is left as it is. Where which card is thrown changes
   what a score reads (`HandParts::card_discards`), a discard throws first the cards whose
   discard alone leaves the best play of the cards that stay scoring most, then the lowest
   (`throw_order`; an effect every card has alike, Green Joker's, changes nothing). One measure,
   the engine's step, for every joker in card.lua's `discard` context (Hit the Road's Jacks,
   Castle's suit, Green Joker's and Ramen's losses, Burnt Joker's level), with no list of
   jokers. So what a dig costs the jokers that read the count of discards left (Banner's Chips,
   Mystic Summit's Mult) isn't weighed, while Green Joker's Mult, which the `discard` context
   lowers, is: counting the count too (the whole step) was measured and not kept, the
   `discards` board (Banner, Mystic Summit) −10.4 / −2.5 / −15.9 points, since only the dig's
   side was charged and playing on spends its discards later too; the fix charges both sides
   what each spends. Raised by the oracle: on a board with Greedy
   Joker, Hit the Road and Joker the oracle won 19 points more, throwing 1.32 Jacks a round
   against the policy's 0.89 with as many discards (it picked the digs that throw Jacks).
   Measured (2026-10-28, `player_win_rates`, 2,000 rounds a board, the boards `hit_road` and
   `castle` added at the half points of the player before): Hit the Road +7.8 / +5.9 / +2.0
   points (4 hands and 3 / 1 discards, 1 hand and 3), Castle +7.3 / +0.2 / +8.6, Green Joker's
   boards +0.6 / −0.2 and with The Hook +1.0 / +0.7, every other board the same round for
   round; the oracle's gap (200 rounds) on Hit the Road 18.5 → 11.0, 8.0 → 5.0 and 4.0 → 2.5
   points, on Castle 7.0 → −0.5 and 11.5 → 3.5. A first version took the ratio the discard
   gives the hand the dig leaves as a factor on every hand of the chase: right for a ×Mult
   (Hit the Road), too much for added Chips (Castle's three Chips a card read off a High Card as ×1.56 where
   the flush the chase completes gains about ×1.12) and for Green Joker's Mult; it won Castle's one-hand board +12.3 but lost
   Castle and Green Joker boards 0.6-0.7 points (the reviewer's finding). The owner's saved
   states: 5 of 135 picks moved, 2 ties and 3 unclear (+0.29% ± 0.87%, +0.37% ± 0.86%, +0.46% ±
   0.48%), none shown worse; the judge plays its futures by this policy, so it partly judges
   itself. No fixture's Best play moved; two shop rankings reorder below their top three, the
   jokers' measures played by this player (selling Hit the Road beside Banner falls 0.954 →
   0.827, the player now feeds it Jacks, and Banner's sale rises past Cloud 9's; on
   `shop_keep_unfaded_ramen` every option ~8% lower, Green Joker and Ramen measured anew). A
   full analysis of a saved state with Hit the Road and Mail-In 4-28% more CPU (more digs, and
   the chase's terms scored again on the dig's board), one without such a joker 3-7% (each
   card's discard tried once a hand, `card_discards`; the same machine busy, two runs each). What's left: burning
   discards while on pace (`burn_pays`) still looks only at jokers that score by discards left;
   burning on the discard's lift was measured the same day and not kept (Castle −10.8 points,
   Hit the Road −1.0: burns throw away digs, as in 2026-10-05's measurement, Hit the Road +1.3
   / +0.5, Castle −5.4 / −1.7); a chase counts its first dig's board, not its later digs';
   the plays scored there are those `b` picks (with Burnt Joker the dig's level can make
   another hand the best: understated); the cards are ordered by each one's discard alone, so
   what a set does together (Burnt Joker's level for the hand the set makes, Faceless's three
   faces) doesn't choose them; a
   junk hand's cards are the lowest whatever their play does to the jokers; cashing paying
   discards is a threshold, and Delayed Gratification's money, which spending a discard gives
   up, isn't
   weighed. What a discard leaves beyond the round's points (`RoundGoals`: the jokers past the
   round, its money) was measured against the oracle by the round's value, on boards with
   goals for Green Joker, Castle, Ramen, Yorick, Delayed Gratification, Mail-In Rebate,
   Faceless and Trading Card (1,000 rounds each, 2026-10-28): the oracle's gap beyond winning
   was 0.0000 to 0.0049 of a round (its win gap 0.8 to 21 points), so weighing digs by the
   round's goals wasn't built: the gap is in the points. What a discard or a hand does to the jokers over the run is
   measured since 2026-10-17 from the simulated player's rounds (`value::Growths`, gap 18:
   Green Joker, Castle, Ramen, Ice Cream, Seltzer and every joker a round changes), not from
   the run's own play: before, a table faded Ramen −×0.3 an ante whatever was played, and a
   run played without discards kept it whole (raised in play 2026-10-11: still ×2 three antes
   after buying it, Green Joker at +25) while By Ante 8 preferred selling it over Wily Joker
   (fixture `shop_keep_unfaded_ramen`). The save counts the run's own discards
   (`unused_discards`, with the rounds played), a direct check on the simulated player's; the
   growers in neither the rounds nor the shop events (Ceremonial Dagger, Glass Joker, Hiker,
   Caino, Fortune Teller, Supernova)
   are valued at their size now and labelled so (`unprojected_growth`; a test keeps every
   growing joker in one of the three). Ceremonial Dagger's is a choice: what you put to its
   right (raised in play 2026-10-09: a Dagger read as no better than taking nothing, unlabelled),
   so it needs a joker's loss as an event the projection can price (Madness's destroyed
   jokers aren't counted either; neither is played as a blind is set, `Board::at_blind_set`:
   tried 2026-10-27 and not kept, since a joker the advice adds goes at the right end, gap 26,
   where a rightmost Dagger ate every buy and the projection's finds (on a fixture with its
   Dagger moved last every shop option's measure fell ~15×), and the growth measure counted a
   grower destroyed as used up while a joker that doesn't grow lasted: the loss priced as an
   event for every joker, and the place a joker is added, come first), and money for how a round is
   played (Delayed Gratification, Trading Card, Faceless Joker) is measured since 2026-10-25
   (a round's money with and without the joker on this ante's plain blinds: `LongRun::income`,
   gap 36). The general fix for the loss: measured from projected rounds (the board at a
   round's end against its start). Not
   modelled in the discard step: Purple Seal's Tarot (raised in play 2026-10-11, run
   BLZMQ7NJ, The Eye: discarding a Purple Seal 2♠ gave Justice, Glass on a Straight's J♦ took
   its last hand from 49,084 to 76,828 and won the round. The Tarot is random, but going for
   it raises the odds: the fix values the discard over the Tarots it can give, each at its
   chance, as the random effects already are (`RANDOM_OUTCOMES`), so a Purple Seal is thrown
   first when a round is short and a consumable slot is free), and what Trading Card's destroyed card
   changes beyond the deck's size (Caino, Glass Joker); in a projected round (`Board::new_round`),
   the targets the game draws anew each round (Castle's suit, Mail-In's rank: this round's are
   kept) and Campfire's reset after a boss.
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
   outlook's heuristics note says growth isn't projected, which is out of date (the projection
   grows jokers: `value::Growths`, `grow_antes`).
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
   changes, DNA and Crystal Ball alike). A round's extras reach the round, the Blue Seals and
   the round's money (`LongRun::income`, measured on this ante's plain blinds: a hand left
   at cash-out, a discard that pays), not the rest that reads today's round: joker
   growth in rounds is measured at this ante's three plain blind sizes with your board as it is
   (`value::Growths`), DNA's opening hand today's hand size (`round_decks`). Projected rounds are
   played out with no target, so a discard
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
13. **The long run doesn't know the run ends.** The projection values money, rerolls and packs
   as if the run went on past Ante 8 (`LongRun`, `Spending`): near the end, on a board that
   already beats the bosses ahead, the shops' worth falls (the survival factors cap at 100%,
   but the long run still counts money), so skips are undervalued and spending overvalued. The
   general fix: the projection's horizon is the run's end (the win ante), and what's left past
   it counts nothing (or what endless play is worth, if that's ever the goal). Raised in play,
   2026-10-05 (Ante 7, Throwback, Handy Tag). Done since (2026-10-05): money tags (Handy,
   Garbage) are valued as money now; the skip line says "tag not valued" and a skip's joker
   growth in plain view, not only in its hover; the skip side meets this ante's boss and the
   next ante's on the board the skip leaves (`Board::after` `SkipBlind`, as Meteor's
   `next_boards`), though the jokers the shops ahead may show are still measured on your board
   as it is (their lift on top of a grown board understated, and said in the tag text); and
   skip-or-play is shown in the shop for the next blind, with the money you have now. Still
   left: the money that run's end makes worthless, which matters most near Ante 8; and in the
   shop, skip-or-play takes the money you have now, not what each shop option would leave you
   (the shop ranking doesn't weigh skipping the next blind). And when the next ante's boss is
   at 0% on both sides, both products are 0 and the verdict is "close" whatever this ante
   shows (seen in the shop fixtures: skip 90% against play 88% this ante, $22 from Handy); the
   shop ranking floors that factor with three wins (`rank_options`), skip-or-play doesn't.
14. **You can't keep a joker out of the sell choices.** Every choice that makes room (a shop
   joker replacing one, a pack's joker, the rescue list, skip-or-play's Buffoon pack) may sell
   any joker that isn't eternal; a joker you're finishing for a reason the advice doesn't
   weigh (a Gold sticker: `missing_gold` is shown, never weighed) can be the one it sells, and
   a pack's joker counts as if you'd make room for it. The general fix: a per-joker "keep"
   choice on the page, kept with the run, that every sell choice respects as it does eternal
   (a pack joker then counts only into a free slot or by selling another). Raised in play,
   2026-10-05 (Ante 7, Arrowhead without its Gold sticker). A keep is more than a sell filter:
   a Gold sticker goes to every joker held when the run is won (misc_functions.lua
   `set_joker_win`), so a kept joker is a condition on the win, and the measure stays the same
   (win chance and strength, with the joker held to the end). Its slot is then taken in every
   plan, and what makes it useful (Straights for Superposition, a Death that fills a rank) is
   worth more because there's no other joker for that slot. That's valued only through win
   chance, never as a goal of its own, so the keep adds no second objective and no "worse play
   for later". Today the advice weighs the slot as free to swap, so it shapes the deck for the
   jokers that do the scoring now. Raised again 2026-10-12 (run on Anaglyph Deck, Ante 2,
   Superposition without its Gold sticker: Death went on a second Steel 4♦ while the owner
   was moving to Straights). Deck shaping's own value is gap 27.
15. **Bosses whose rule isn't in the simulated round are played as plain blinds.** The
   simulated round knows the suit and face debuffs, The Pillar's and Verdant Leaf's (below), The
   Manacle, The Needle, The Water (set as a fresh round's blind is: `Board::at_blind_set`), The Hook (2 held cards discarded at random
   once a hand is played, before it scores: `sim` `boss_takes_held`, `Board::discard_by`) and
   the scoring bosses (`engine/score.rs`); the rest only change their target (said as "not simulated" on the
   blind). The Hook is in the engine's `score` (its press-play step, `score::pressed`): once the
   played cards leave the hand it discards 2 held cards through `Board::discard_by`, before the
   hand scores, and which ones is a pick (`Rolls::pick`), asked only when it can change the hand
   (`hook_pick_matters`: a held card the score reads, a discard joker that reads the cards). An
   estimate averages every pair it could take (the floor a play is chosen by, `Play::mean`
   and pace, the big-hand shortlist, the score Best play shows), and what it surely
   scores is the pair that costs most (`Outcome::sure`, `Play::sure`: the finish, "play it now",
   Best play's "wins now"); every roll for you takes the pair that costs least.
   Why the average (2026-10-15, once D15): which pair is taken isn't a roll that fails, every
   pair is as likely; with the costliest pair as the floor a held card worth anything was always
   the one taken (Baron and a Steel King held: both Kings played, floor 108, over a Pair of 7s
   that holds them, floor 48, expected 116; Baron with Shoot the Moon's best floor 960 → 360).
   The costliest pair answers only "does it surely win", and any pick over sets of cards is
   averaged the same way. Revisit if the Hook boards or their gap to the oracle get worse than
   with the costliest pair, a Hook round is lost on a finish that won only on average, or the
   cost (every pair scored, ~16× a best play; the held-card measure below about C(n,2) + the
   pile's kinds × C(n,3) for n held) shows in the timings. The round makes
   its own pick (`boss_takes_held`: each held card keyed at random from a seed of its own, so a
   play leaves the round's draws as a discard does and moves holding the same cards lose them
   alike) and scores what's left (`score_pressed`). A win now's value (`finish_value`) averages
   every pair whole: its money, growth and what the cards left pay at round end (`held_at_end`,
   `over_hook_sets`, also in `play_on_instead`'s gate). An eaten Ramen leaves the board after the
   hand (`Outcome::eaten`). Not carried to the board after the hand in an estimate: Yorick's
   count and a card Trading Card destroys. What a held card adds (`held_value`), when the score
   reads held cards, is measured against a card drawn from the pile in its place, over every set
   the Hook could take (a discard draws back to hand size: the Hook picks from as many cards,
   and the card counts only when it stays, with the cards it may stay with); none when it takes
   every card held. A junk hand played to dig is a played hand: the chase goes on past one only
   with the chance the Hook spares every card the plan keeps (`chase`, `ChaseRound::takes`).
   The averaged outcome takes its score, money and each
   joker's numbers from every pair; whether the round can be won surely now is the highest
   `sure` of any play (`Play::surest`), and without goals the finish is then the best floor that
   surely wins (`surely_winning`). Where the pick matters a play's chips and Mult no longer
   order it, so `playing_on`'s later hands search in full (`front_holds`). Not yet: the
   look-ahead's later hands keep the cards The Hook took (the cards held stand in for the ones
   drawn); a golden case under The Hook would need the cards it took recorded (`Given`): its
   rescore is the average; on the last hand a play that wins on average but not surely is
   weighed only by its average, not by its chance over the pairs; without The Hook a card is
   still measured left out, not against a card drawn (Raised Fist: a 9 beside a King isn't kept,
   as leaving it out lets the Fist read the King; the drawn-card measure was tried on every
   board and lost, below); the junk hand's size isn't a move (with 8 cards and none
   kept for its held value a plan of 2+ kept cards is never spared, where playing 1 card and
   holding 7 would be 10 times in 21), a plan that loses a card to the Hook isn't resumed, and
   the cards drawn for the ones it took aren't counted toward it.
   Measured 2026-10-15 (`tests/player.rs`, boards with The Hook; 2000 rounds, targets the
   player's at 04410bb): held_hook 50.0/50.0/50.3% → 48.2/58.1/61.8% (4h3d, 4h1d, 1h3d),
   enhanced_hook 50.0/50.1/50.7 → 49.9/50.2/51.6, growers_hook 50.0/50.1/50.5 → 50.5/48.6/50.5;
   the gap to the oracle over the nine 85.0 → 80.5 points (200 rounds each, ±3-5 a board); every
   other board the same. Two boards lower: on rounds 2000-22000 (paired with the player at
   04410bb) held_hook 4h3d was level (−0.17 ± 0.45: the 1.8 points on the benchmark's 2000 were
   noise), growers_hook 4h1d wasn't (−2.36 ± 0.28): the player played twice as many junk hands
   (0.31 → 0.64 a round), each a dig the Hook broke by taking the plan's cards, which the chase
   didn't count; it had chased less before only as its pace counted a Green Joker Mult a hand
   the Hook's discard takes back. With the chase counting the Hook and `held_value` measured
   against a card drawn (2026-10-15), over the same rounds against 04410bb: held_hook
   +3.50/+12.02/+13.53 (±0.43-0.46), enhanced_hook +0.62/+0.33/+1.49 (±0.12-0.24), growers_hook
   −0.48 ± 0.30 / −0.38 ± 0.27 / 0, with no discard +1.13 ± 0.17 (no junk hand played under the
   Hook any more). On the benchmark's own rounds (2000, the targets above): held_hook
   48.2/58.1/61.8 → 54.4/62.1/65.5%, enhanced_hook 49.9/50.2/51.6 → 50.2/50.8/51.6, growers_hook
   50.5/48.6/50.5 → 50.4/49.5/50.5 (the player at 04410bb: 50.1 on growers_hook 4h1d), every
   other board the same; the gap to the oracle over the nine 80.5 → 86.0 points (held_hook
   10.5/11.5/11.0 → 18.0/7.0/13.0 for 1h3d/4h1d/4h3d, growers_hook 4h3d 7.0 → 4.5, the rest
   within ±2; ±3-5 a board, the oracle's futures played by the better policy too: on held_hook
   1h3d and 4h3d it gained 11 and 9 points to the player's 3.5 and 7, so its choices there are
   the next place to look); 3 of 367 saved picks moved (2
   tie, 1 unclear: the Hook fixture `boss_hook_ante1`, whose pick is within 1% of the reference's
   best before and after); a round on a held_hook board about 10-20% slower. growers_hook's rest
   is gap 8: the player now spends more discards (2.60 → 2.71 a round with 3, 0.85 → 0.95 with
   1), each a Green Joker Mult the dig doesn't weigh.
   Against a card that adds nothing in its place (the hand keeping its size) instead of a drawn
   one, held_hook was level or lower (−0.08, +0.47, −2.25 ± 0.3-0.4): that stand-in made every
   card Raised Fist could read a keeper.
   Tried 2026-10-16 and not kept: the drawn-card measure on every board (the same code with no
   set taken, gated on `score::held_read` as under the Hook). Only the held boards move, all
   lower: over rounds 2000-22000 paired with the player before, −2.76 ± 0.23 / −3.60 ± 0.26 /
   −7.98 ± 0.33 points (4h3d/4h1d/1h3d; the benchmark's 2000 rounds 92.0/81.7/50.8 →
   89.1/77.3/43.5%), every other board the same (the enhanced deck's boards, whose Steel cards
   are read, round for round); the gap to the oracle on the held boards 1.5/2.0/2.5 →
   4.0/4.5/13.0 points (200 rounds, ±1.3-3.2; the oracle itself lower, its futures played by the
   same policy); a round on the held boards about 1.5× as slow (the benchmark's 6.4/5.2/3.7 →
   13.1/9.9/8.5 s on a busier machine, its unchanged Hook boards 1.4× slower in the same run).
   On scratch boards (rounds 2000-12000, targets by the player before): Raised Fist with Joker
   +12.0/+2.5/+19.2
   (± 0.6), Baron with Shoot the Moon +0.0/+0.1/+0.6 (± 0.2), Blackboard with Joker
   +0.8/−0.2/+0.7 (± 0.5). Why the held boards lose: beside Kings and Queens the Fist reads a
   Jack, a 10 or an Ace, and every card held above the lowest became a keeper, since a card drawn
   in its place is lower on average. The measure replaces one card with the rest held as they
   are, but a dig throws up to 5 together, and the lowest of their draws is nearly always
   below the Jack: the keep protects little, and it takes the slots the dig for Kings and
   Queens draws into. A keep is all or nothing (never thrown), whatever it's worth beside what
   the dig is for, so a small, real held value blocks a large dig. The Best play picks of the 18
   replay fixtures stayed (the joker pool's, tarots' and outlook's values moved, through the
   held-card jokers they simulate). Tried again 2026-10-16 with the other held cards a dig throws
   drawn too (up to 4 beside the card): held 1h3d −12.5 ± 0.4 points (20,000 rounds paired), the
   same failure; under The Hook it was also 0.3-0.8 points below the drawn-card measure. And a keep
   weighed against what the dig gains (each plan's dig also trying to throw the least-worth
   keepers, the chase taking the best, later hands and the hand left charged what the cards
   thrown were worth held): it lost to keeping them on four-hand rounds (−0.3 to −1.8 points
   paired) and gained on some one-hand rounds; played on from the decisions where it threw a
   keeper (Baron with Shoot the Moon, 4h1d, 300 futures each), throwing lost 6.7 ± 1.4 points of
   win chance where the chase expected 3.5% of the target more, mostly junk-hand digs: the
   chase's points aren't fine enough to trade a held card against a card more drawn. Built
   instead (2026-10-16): when the score reads held cards, a dig plan's completion and the play of
   the hand its first dig leaves are scored with the cards the hand holds then (the plan's own
   cards beyond five, the cards worth holding, what the dig leaves, up to the hand's size, and
   cards drawn into the slots left: `held_beside`, `held_draws`, `HELD_DRAWS`), a lesser plan
   inside a chase in the chasing plan's hand (`lesser_of`); and the cards held back for their
   held value take the slots a dig would refill on every board (`ChaseRound::stays`, before only
   under The Hook), a plan that doesn't fit beside them having no odds (`aim_odds`). The keep
   itself stays all or nothing, measured as above. Over 20,000 rounds paired with the player
   before (rounds 2000-22000): held +4.2/+4.6/+2.6 points (4h3d/4h1d/1h3d), held_hook
   +16.9/+5.9/+10.5, the enhanced deck's boards +0.1 to +0.7, enhanced_hook and trousers_goad
   level, growers_hook the same round for round; on scratch boards (targets at the half points of
   the player before) Baron with Shoot the Moon +9.5/+6.1/+4.6, Raised Fist with Joker
   +6.6/−1.7/+9.1, Blackboard with Joker +5.2/+0.7/−0.8, under The Hook Raised Fist
   +21.5/+5.8/+13.9, Baron with Shoot the Moon +3.0/−0.1/+3.4 (± 0.04-0.43; the two lower: gap 6).
   The benchmark (2,000 rounds) every board level or better, mean 59.16 → 59.78%; the
   gap to the oracle over the 15 boards whose score or deck reads held cards 93.5 → 74.5 points
   (200 rounds each, ±0.5-4.8 a board: held_hook 13.0/7.0/18.0 → 0.0/5.0/12.0, the rest within
   ±2; on Raised Fist with Joker 4h1d +4.0 → +7.0, gap 6).
   No replay fixture's Best play pick moved; 40 of 387 saved picks moved (5 better, 20 tie, 15
   unclear, none worse). A round on the enhanced deck's Hook boards about 1.5× as slow, the
   others about as before. Revisit when the chase predicts its round better (a keep weighed
   against the dig could then be tried again) or the oracle throws cards this keeps.
   The Tooth is in the round's outcome only (its money leaves the board, not the round's
   value: `Outcome::cash` counts money made), and the blind says so. Since 2026-10-27 Chicot
   disables a boss as its blind is set in every round ahead (`Board::at_blind_set`, its
   `setting_blind`; the row then names nothing unmodelled), Luchador's sale still isn't a move;
   a round in progress replayed from its start (`fresh_round`) keeps the save's disabled blind
   (before 2026-10-27 it was replayed active). The general fix for what's left: the rest of the
   game's press-play step (blind.lua `Blind:press_play`: The Tooth, Crimson Heart, The Fish) in
   the engine's `score` as The Hook is, with The Tooth's money in the round's value. Raised in
   play 2026-10-09 (Ante 1, The Hook not simulated). Best play's search against the reference
   on that Hook state (fixture `boss_hook_ante1`): two seeds of three pick within noise of the
   best (0.7%, 1.1% ± 1.3%), the third a Two Pair 5.1% ± 1.5% below it, the best moves all cut
   by the budget after 32 rounds. Keying the Hook's pick by card rather than by position didn't
   change it (4.8% before), so it's the search's first cuts under the Hook's added spread: a
   question for the noise stage (`SEARCH_BUDGET`), measured on this fixture; with the budget
   halving many moves (2026-10-11, gap 1) every seed picks within 1% of the best (0.73% ± 1.08%
   on seed 44).
   The Pillar and Verdant Leaf are in the simulated round (2026-10-27), through one port of
   blind.lua `Blind:debuff_card` in the engine (`Board::debuff_cards`, which `at_blind_set` and a
   sale call on every card, as `Blind:set_blind` and `Blind:disable` do): while the blind is
   active, a card of its suit, a face card (The Plant), a card played this ante (The Pillar) and
   every card (Verdant Leaf). No other card rule of it is used by a vanilla boss (`debuff.value`
   and `debuff.nominal` are set by none), so the port is the whole of it for playing cards.
   The Pillar reads a card's own state, `Card::played_this_ante`: the save's
   (`ability.played_this_ante`, which state_events.lua `G.FUNCS.play_cards_from_highlighted` sets
   on each card played and `end_round` clears on every card once a boss is beaten), and the
   board's simulated rounds': a round adds the cards it played to `Board::played_ante` as it ends
   (`sim` `play_round`, kept out of the boards its choices try), and a fresh round's start marks
   each on a card like it not marked yet (`at_blind_set`), so the rounds chained through the
   blinds ahead (`sim::chained_runs`, `value::Ahead`) meet The Pillar with what the Big Blind
   played debuffed; a boss beaten ends the ante (`Board::won_round`: the board's cards cleared,
   and a fresh round's start read with none played, `Board::ante_over`). A round in progress at
   The Pillar plays the save's debuffs.
   Verdant Leaf: while a sale would lift the blind (`Board::sale_lifts_blind`), the simulated
   player weighs one before each hand (`sim` `leaf_sale`), and Best play's first move is played
   on the board that sale leaves, as the rest of its round is (`outcomes_after`: else a discard
   got the sale and a play didn't): the joker whose sale leaves the round scoring most by the policy's own pace (`playing_on` over the
   hands left), from the hand held and from `LEAF_DEALS` (8, a budget) deals of the round's cards
   (hand and pile, in a fixed order, on a seed of their own), when that beats keeping them all;
   equal ones by the most money, then by key. Never an eternal joker. `Board::sell_joker` pays
   its sell value, grows Campfire (`selling_card`, not a debuffed one) and disables the Leaf, and
   the cards' debuffs are set again. Not modelled: what a joker sold does itself (`selling_self`:
   Luchador disabling any boss, Diet Cola, Invisible Joker) and what it gave while held
   (`remove_from_deck`: Juggler's, Troubadour's, Turtle Bean's, Merry Andy's and Stuntman's hand
   size, Drunkard's and Merry Andy's discards: every sale is weighed at the same hand size, gap
   22's class); `sell_joker` is where Luchador's sale would go, with the rest of `Blind:disable`
   (The Needle's hands, The Water's discards, The Manacle's card) applied to the round. The
   choice is by points: jokers that add none tie and go by money, then key (Mr. Bones' save and a
   round's end money aren't weighed; the round's value counts a sold joker's loss only for the
   jokers a round changes, `RoundGoals::jokers`: as a move, a sale would be valued as the
   projection values one let go, `Fate`). The choice is remembered per board, hand and cards left
   (`leaf_choice`): every round of a start whose hand is dealt makes the same one, so Best play's
   moves and a round in progress's runs weigh it once a thread.
   Why the pace over deals, not the hand's best play: on the saved Verdant Leaf state P6TPNC3T
   (Swashbuckler, Shoot the Moon, Blueprint, Vampire, Card Sharp, Ante 8) the hand held's best
   play sold Shoot the Moon (no Queen in hand, five in the pile: 2.3% to win), its best play over
   deals Card Sharp (whose ×3 needs a hand played again: 0.7%), where selling Swashbuckler wins
   57%; the pace over deals sells Swashbuckler. And keeping them all can be best: with Duo, Card
   Sharp, Gros Michel and Ride the Bus the debuffed cards' hands still score (GA74B7U3, Ante 6:
   83% keeping them all, 31% for the best sale).
   Measured 2026-10-27 (300 rounds each). The Pillar: on the 103 saved Small and Big Blind
   states with it ahead (15 runs), the boss alone from your deck as the save leaves it moved by
   more than 0.02 on 38 (mean −0.049; QBJCMCRM's Big Blind, 5-14 cards played, 0.43-0.50 →
   0.14-0.21; I6XKKH65's Stone deck, 15-27 played, 0.73-0.79 → 0.45-0.57), and the Small Blind's
   chain through the Big Blind into it (the Big Blind's cards carried) 0.12-0.28 lower than with
   The Pillar played as a plain blind wherever it was below 99% (Ante 1: 0.75-0.84 →
   0.53-0.69). Verdant Leaf: on the two saved states in it the player's sale is the best of
   every joker's (60.7%, 57.3%); on 40 Ante 6-9 boards from the saves played at Verdant Leaf from
   a fresh round, 0.012 below the best of every sale and keeping them all on average, 3 more
   than 0.05 below it (two where keeping them all was best, 0.31 against 0.15 and 0.83 against
   0.74; one where Card Sharp's sale was, 0.51 against 0.39): the pace's later hands are the
   cards held again (gap 6), which counts a repeated hand's Card Sharp and a sale's chips past
   what the round sees. Racing each sale on rounds of its own would close it, at about 6 × 32
   rounds per decision where the pace takes (1 + 8 deals) × (1 + the jokers sellable) paces.
   The parity tier (D17, against 2026-10-27's baseline): `Board::at_blind_set` 532 → 548 of 576
   the same, the known gap 15 group 21 → 4 (Amber Acorn, Crimson Heart; one Pillar setup still
   differs by a `setting_blind` joker), nothing new; The Pillar and Verdant Leaf left the test's
   known group (`GAP15_BOSSES`).
   Left, Best play's side (it needs the valuation, left alone while gap 17 was worked on):
   Best play's own play in the Small and Big Blind doesn't count what its cards cost at The
   Pillar (`value::Ahead` measures the blinds ahead from your board as it is, not from the cards
   a line leaves played), so fixture `ride_the_bus_queens_dig` (Small Blind before the Big Blind
   and The Pillar; the owner's line keeps the Queens out) still waits in `pending_expect` on this
   gap; selling isn't a move of Best play's at Verdant Leaf: its moves are played after the
   simulated player's sale and the blind names it ("not simulated: … assume selling X first"),
   but Best play neither races the sale nor shows its moves on the board it leaves; a round in
   progress at The Pillar replayed from its start (`advise.rs` `fresh_round`) counts the cards
   this round played as played before it (said on the blind). A replay from the start keeps a
   blind its round disabled (`fresh_round`: Chicot, Luchador's or the Leaf's sale; before, a Leaf
   already lifted was replayed active and another joker sold). The
   simulated player plays the Small and Big Blind without keeping cards for The Pillar. The
   boss's own row ahead (and the shop ranking's, and the shops' find `value::Ahead` sizes on the
   boss alone) counts only the cards the save shows played, not those the blinds before it will
   still play (said on the blind: "not simulated"); only `Ahead`'s chain carries the Big Blind's.
   Also not yet: a card whose enhancement changes (a tarot, Midas Mask, Vampire) loses
   `played_this_ante` in the game (card.lua `Card:set_ability` rebuilds `ability`), not in the
   engine; a card a held consumable changed in a simulated round is carried as it was changed;
   Campfire's reset at a boss's end (gap 8), which a Leaf sale's growth now reaches. Raised in
   play 2026-10-11 (run I6XKKH65, Ante 5 shop, a Stone deck at 56% for The Pillar) and 2026-10-05
   (Ante 8, Verdant Leaf, Arrowhead kept for its Gold sticker, gap 14). Seen in Best play
   2026-10-05: on Ante 8's Small blind every move wins, and the Blue Seal planets a line keeps
   are valued only by their gain in a long run past the run's end ("Ante 10 boss", gap 13) on a
   final boss at 100%, so lines making 0.64–0.71 planets tie, where after the sale at Verdant
   Leaf each Flush planet is worth about a tenth more points: the long run measured to the final
   boss (gap 13) is what's left there.
16. **What isn't valued ranks as worth nothing, and an empty slot counts as filled.** Effects
   the valuation doesn't cover (Ectoplasm, room in the consumable slots, some tags and
   vouchers) say "not valued" and add nothing to the ranking: a person reads the label, a
   player that takes the top option never picks them. And a choice is valued by the board it
   leaves (simulated rounds and the long-run projection), where empty joker slots are filled
   with stand-ins for the jokers the shops ahead would sell, even when no shop is left before
   the boss: a slot is worth a stand-in after a skip that leaves no shop, and nothing for what
   it would actually hold (a cheap joker to sell at Verdant Leaf, gap 15). The general fix: each
   "not valued" effect through the one measure, and stand-ins only for the shops still ahead
   (with gap 13's run end). Raised 2026-10-05, thinking of the advice playing on its own
   (balatro-agent). Likewise what a reroll may find is the pool's jokers without editions
   (`shop_pool`, `money_value_with`): a Negative one (0.3% a joker card, poll_edition, not
   raised by Hone; not checked against the game here) is a joker and a slot, worth little on
   average but much where a slot is (a joker to sell at Verdant Leaf, with a joker kept: seen
   in play the same day, a Negative joker and an Ectoplasm both in Ante 8's shops). A joker's
   round money is valued since 2026-10-25 (measured, gap 36); a joker whose role is money and
   whose rounds pay none (Gift Card, Credit Card) still says "not modelled" when sold.
17. **The money's finds add every joker's gain, count a full board's swaps in two units, and the
   judge can't play tarots.** Since 2026-10-26 what an option's money left buys
   (`survival_with`, `money_value_with`) is the money spent shop by shop as a player buys, on
   2000 walks: in the shop you're in only its rerolls (its cards are the options), then before
   each shop after a blind its reward, a hand's money and the interest (a debt paid back first:
   it no longer counts as $0), its free cards, its rerolls; a joker is bought from the shop it
   shows in when it adds to what's kept, never from a shop already left, and the rerolls a shop
   are a policy chosen on half the walks and valued on the other half (up to 8). The walk starts
   from the board each option leaves (`Start`): its own chance the floor a find must beat, a
   joker bought or a Buffoon pack's pick taking one of your free slots, a sale freeing one; and
   since 2026-10-27 a find goes in a free slot or in place of the joker whose loss costs least,
   a joker of yours (its contribution read from the contribution pass as a sale's is; not an
   Eternal or a Negative one; its sale's money paid) or the one the option buys, so a full board
   swaps as a player sells for a better joker. The jokers kept count as score gains added on
   your board's scores in that round (`Kept::Chances`, `Scores`: a lognormal through its p10 and
   p90, shifted to its simulated chance; each joker's multiplier the one its own simulated
   chance gives there, `Scores::ratio_for`), never under the best one's own chance; an option
   with no simulated chance at the next ante's boss (packs, planets, tarots) gets one read off
   its scores there, a reroll your board's. Leaving walks out of the shop; a reroll is its
   money's own chain of rerolls, this shop's next one first (`Stay`); next-ante strength counts
   the money's finds for every option, keeping the money ×1.00.
   Measured on 25 of the 144 shops below (`tests/shop_playon.rs` `money_finds`, the states in
   `tests/fixtures/private/money_finds_states.txt`: the advice's leaving against the judge's
   leaving played on with up to 8 rerolls a shop, "spend8", the walk's budget;
   `BAV_FINDS_QUICK`, about 35 minutes): this boss 0.018 high (mean absolute error 0.023), the
   next ante's boss 0.021 high (0.046), against the commit before's 0.036 high (0.044) and 0.062
   low (0.128); the judge's "cap" policy buys less, so the advice reads 0.10 high against it at
   this boss. The versions were compared in an earlier setup (the better of the judge's two
   policies with this shop's rerolls taken first, the advice's leaving made to take them too by
   a switch since removed): the pool screen has no winner's curse on average (2026-10-25: the 12
   jokers screening best simulated again on 400 fresh rounds gained as much; only the Ante 1
   empty boards leaned high), so the old finds' lean was how money was spent. On the 22 of them
   without Chaos the Clown (the judge played it only later): the old sum this boss 0.033 high
   (mean absolute error 0.043), the next ante's boss 0.071 low (0.122: one joker counted where a
   player stacks several, a debt as $0, income spent before it's earned); the walk with gains
   multiplied and no swaps 0.024 high (0.035), 0.025 low (0.090); with swaps, multiplied 0.027
   high (0.036), 0.028 high (0.046), and as kept, added, 0.023 high (0.034), 0.026 low (0.051).
   Multiplied, swaps stack small finds on a hopeless board into a near-sure pass (I6XKKH65
   a4-r7, 7 of 7 slots, The Plant at 0.007: leaving 0.71 and above Mime, which the judge puts
   +0.39 above leaving at this boss), so the gains add, as most jokers' Chips and Mult do;
   without swaps a full board took one find at a time (RB51DNGQ a3-r6: the next ante's boss 0.27
   against 0.91 played on; with swaps 0.94).
   How the picks were judged (to redo it): the 144 shops are
   `tests/fixtures/private/shop_judge_states.txt`; each version's top option per shop from
   `balatro-advisor --json analyze --file <state>` (`options[0].label`); each moved pick turned
   into a judge option (a shop joker `joker:<key>[:<edition>]`, a sale `sell:<key>`, `reroll`, a
   Buffoon pack `pack:<2 or 4>:<price>[:2]`, a Celestial pack `celestial:<3 or 5>:<price>`, a
   planet `planet:<hand>:<price>`; leaving is the zero every option is played against) and
   played by `shop_playon` (`BAV_PLAYON`), the mean of its two policies' "both bosses" lines; a
   move better or worse where the difference is over two standard errors. Top picks on the 144
   shops against the commit before (51dbc73, the judge playing each option against leaving on
   the two bosses' joint chance, the mean of its two policies; the judge now also plays planets,
   Celestial packs, a sale, a Mega Buffoon's two picks and Chaos the Clown's free reroll): 45
   moved, 28 judged: 1 clearly better (a reroll → a Celestial Pack, +0.05 ± 0.02), none clearly
   worse, 27 within two standard errors, summing +0.11 (17 up, 11 down; the largest down within
   their noise: Chaos the Clown (Foil) → a Buffoon Pack −0.10 ± 0.08, a reroll → a Jumbo
   Celestial Pack −0.09 ± 0.11; Mime stays the pick above); 17 can't be judged, most a reroll →
   an Arcana or Spectral pack, the rest tarots, Seed Money and planets over an Arcana Pack.
   Tried on the same shops and not kept: the walk alone with finds added to each option's own
   chance (13 judged, −0.18: a pack's or reroll's own next-ante chance wasn't counted); the walk
   with the earlier fix but not the option's board (23 judged, −0.17); the option's board with a
   last-slot buy only replaceable itself and a sale freeing a slot, but no swaps of your jokers
   (35 judged, +0.07 but −0.64 over 17 against the version before it: sales picked over buys,
   e.g. selling Ice Cream over a $1 Smiley Face (Holo), −0.04 ± 0.02); swaps with the gains
   multiplied (29 judged, +0.10; Mime dropped below leaving, above); and the review's accounting
   on top of the kept version (every joker's share in your board's units, the option's board
   added to rather than multiplying the finds', a pool joker simulated in place of one of yours
   given that one's share back, so the walk doesn't take a joker away twice on a full board;
   leaving's next-ante chance your board's own; a find swapped out sold; skip-or-play and
   `Ahead` given your jokers to swap): the finds measured the same (the next ante's boss 0.024
   low, 0.050), but 29 judged picks −0.09 against the commit before and −0.12 over the 4 that
   differ from the kept version (Hanging Chad → leaving −0.14 ± 0.07), and Mime → selling
   Fortune Teller; right in principle, it moves picks the wrong way, likely because the double
   count on a full board offsets finds that read high there. The version before the review's
   fixes (the option's board, a buy on the last slot given a free sixth slot, a money tarot's
   money counted twice) judged +0.84 over 27; against it the kept version moves 14 judged picks
   −0.54, most by the long run deciding once survival saturates (RB51DNGQ a3-r6: every option's
   next-ante survival 0.95-0.96, so Green Joker's long run ×1.65 beats The Tribe's ×0.97, which
   the judge's two bosses put +0.07 ± 0.05 ahead) or within their noise (8I93CDBG a3-r6 with
   Chaos the Clown, Wrathful Joker → reroll, −0.16 ± 0.11: the lean below).
   Left: on a full board a find's multiplier is already net of the joker its simulation replaced
   (`evaluate_candidate`), and the walk takes one of yours away again, while the option's own
   board multiplies the finds' gains (`Kept::together`): two unit errors the review's accounting
   fixes (above, not kept), which offset something else there; leaving's next-ante chance (and a
   pack's, a planet's) starts from your board's read off its scores, which at a chance of none
   or a sure pass is up to half a round off its simulated one (its own simulated chance is the
   general fix, in the review's accounting above); the gains added are right for +Chips and
   +Mult jokers within their term, not for ×Mult ones or Chips against Mult (multiplied, they
   read high); multiplied they lean high (gains are neither); a narrow-spread board needing
   every score ×2 or more still leans high (8I93CDBG a3-r6: +0.72 against +0.24 played on, by a
   judge then without Chaos the Clown); the judge plays no tarots or tarot packs (most unjudged
   moves are rerolls → Arcana or Spectral packs); the walk's own chances still read the screen
   (60 rounds, the next ante's boss on 20); a sale's or a buy's score ratio carries the
   contribution pass's noise (a test near-tie moved within the tie margin: gap 7); a hand's
   money is added once per blind, not per hand left; a swap's sale money comes after the find is
   paid for, not before; By Ante 8 values money as main-hand levels below the interest line +
   $30 while survival values it as jokers found (two models of what money buys, multiplied in
   the ranking); skip-or-play, `Ahead`, `Spending` and `money_value` walk your board with no
   jokers to swap and, on a full board, one free slot it doesn't have (`Start::BOARD`); an
   option's change to the board is read from its kind (a tarot that makes or destroys jokers,
   Judgement, Wraith, The Soul, Ankh, Hex, and Antimatter's slot aren't seen); skip-or-play has
   no next-ante strength.
   Earlier (2026-10-24): no over-buying on the owner's Gold Stake runs that ran low (144 first
   shops of each round at Antes 1-5 of 15 runs: joker buys +0.18 of the bosses' joint chance
   against leaving, Buffoon packs +0.04, the top rerolls a wash costing $3), and three fixes of
   the old sum not kept against finds that leaned high.
18. **Growth is played for at Best play's own move, the simulated player's finish and its plays
   that cost no pace, in its digs only as this round's points (gap 8), and a joker the projection replaces counts only in this ante's blinds ahead.** Since 2026-10-14 a round's value
   (`RoundGoals::value`) counts what its hands and discards leave the jokers a round changes,
   each by what one more Mult, ×Mult or Chip adds by Ante 8 (`LongRun::joker_worth`, in the
   register), so Best play weighs a hand's growth against its points: with Square Joker (+4
   Chips for each hand of exactly 4 cards, for the run) it no longer adds a fifth card that
   doesn't score, and on a round it wins anyway it plays a 4-card hand that doesn't win yet to
   grow it twice (raised in play 2026-10-05 and 2026-10-10; fixture `square_four_card_two_pair`);
   Green Joker's +1 a hand and −1 a discard turn digs into junk hands, and Obelisk's reset on
   your most-played hand is avoided while the round stays won. What's left:
   - Since 2026-10-14 (the same day) the simulated player plays toward the round's value too,
     where a win is on the table: its finish is the winning play worth most (`finish_play`: of
     the plays `best_play` tries that surely win, the most by `finish_value`, which counts what it
     leaves the jokers, the money it makes and the Blue Seals and Gold cards left in hand; the
     highest floor on a tie); and it looks ahead (`play_on_instead`) at the play that doesn't win
     yet and grows the jokers most (`growth_play`, topped up with fillers when they leave its
     growth and the winning cards alone), when that growth, plus what winning now takes off the
     jokers joker by joker (a reset playing on spares; since 2026-10-22), is worth more than a
     spare hand's money by 1% of a won round, and at keeping every card that pays at round end (not only
     those the finish keeps), winning now valued on the same 8 futures as playing on and an
     alternative taken only when clearly better, paired (`paired_pick`, the oracle's test). This
     replaced two rules: the finish kept those cards only when a win without them was there
     (`win_keeping_seals`, still the player's own without goals: the player benchmark, which has
     none, plays exactly as before), and the look-ahead opened only for the Blue Seals the
     finish kept. Measured by `player_values` (fixed goals on 5 boards, 2,000 rounds each, paired
     against the player before): the round's value up on 9 of 10 rounds (growers +0.0048 and
     +0.0035, Blue Seals and Gold +0.0082 and +0.0052, Square +0.0012; Spare Trousers with 3
     discards within noise), the win rate up on the seal boards (+0.75 ± 0.19 and +0.45 ± 0.15
     points) and nowhere down by more than a round in 2,000; a round 3-13% slower (the finish
     shares the decision's search, `HandParts::plays`). Tried and rejected the same day:
     weighing a move that doesn't win but spends a card that pays at round end against the line
     that keeps such cards out of every move that doesn't win (8 futures each): no gain (0.5550
     against 0.5551 without it) and the seal boards 18-20× slower; keeping them out as a fixed
     rule: the seal board's win rate 55% → 9%. On the fixtures, head_keep_spades_and_seal moved
     to a junk play (gap 34). Since 2026-10-17 the plays that don't win weigh growth too
     (`spare_instead`): with no win on the table, the policy's play (the best play, or a junk
     hand that digs) gives way to the highest-floor play that grows the jokers by at least
     `LOOKAHEAD_MIN` more, when the round played on after it keeps pace and scores at least what
     it would after the policy's play (`line_pace`: each once, then the best play on the board it
     leaves and playing on from there, `playing_on`, the one pace every decision uses, so its
     first hand can cost pace the rest makes up; until 2026-10-22 the same cards again every hand, which turned a play that spares a reset once down whenever repeating
     it fell short; so what a reset or a growth does to this round's later hands is in it; a
     budget, in the register)
     and 16 paired futures show it worth more by `RoundGoals::value`. One test for whatever
     joker the goals count: Ride the Bus spared a scored face card, Obelisk its reset, Square's
     4 cards, Spare Trousers' Two Pair (`sparing_play`, which the oracle tries too, so the
     history judge plays on with it). `player_values`: Spare Trousers +0.012 and +0.018 of a
     round (win +1.1 and +1.7 points: the Two Pair grows the Mult the later hands score with),
     growers +0.001, nothing worse; on the owner's saved states two Ride the Bus picks moved from
     a discard to a junk hand with no face card (the judge: ties, +0.4%; it plays on with the
     oracle, which now tries the sparing play too, so it partly judges this change itself). Tried first and not
     kept: the pace test alone, without futures (the Square board lost 7.6 points of win rate),
     and the futures without the "as much as the policy's play" test (with 8, Square −0.003 of a
     round on noise; with 16, a quick pass on a Square board 6 → 20 s). Still not done: the digs are chosen by points (a
     chase doesn't weigh growth past the round, nor a discard's: since 2026-10-28 a dig counts
     what its discard does to this round's later hands, gap 8, where the oracle's gap by the
     round's value was measured to lie; a junk hand's cards are the lowest,
     whatever they reset (tried 2026-10-28 and not kept: once the plan is picked, the throw of as
     many of the cards it may throw whose step the goals value most, jokers and money, by
     `LOOKAHEAD_MIN`, never scoring less or leaving less that pays at round end: on top of
     `throw_order` no `player_values` board moved, the castle board included; before it castle
     +0.0026 ± 0.0013 of a round with 3 discards, all from throwing the suit, which `throw_order`
     now does by points; and a junk hand giving way to its largest part that grows the jokers
     more, Ride the Bus spared a lone King, on 16 futures and screened against the dig's own
     line: growers +0.0006 ± 0.0004 and +0.0010 ± 0.0005, Square and seals_growers +0.0004 at
     most, for simulated rounds 30-80% slower where junk hands dig, as nearly every one passes
     the screen; a cheaper test of which parts are worth the futures comes first); nor the cards that pay at round end, which stay out only while the
     rest keeps pace); a play that gives up pace for growth isn't weighed (with the test at 80%
     of the policy's pace growers gained +0.002 more, and a Square board's quick pass went 6 →
     16 s; raised in play 2026-10-17, fixture `psychic_kings_zero_score`, pending: Ride the Bus
     at +33 under The Psychic, hand K♠ K♣ 5♠ 4♠ 4♣ 3♥, Best play spends a hand on K♠ K♣, which
     scores 0 and leaves Ride the Bus alone (a blocked hand skips the jokers' before step), over
     the owner's Pair of 4s with a King kicker (+1 Ride the Bus and Green Joker now): after the
     Pair of 4s the simulated player resets Ride the Bus in 17% of rounds against the oracle's 3%,
     so the policy's value of that line is 1.001 against the oracle's 1.049 (the Kings' 1.033
     and 1.041); judged with the oracle playing on, the Pair of 4s is +0.76% ± 0.26% over the
     Kings; every move Best play shows there is inside its 1% tie margin, and the oracle's
     +0.76% is too, so the pick follows the policy's estimate on noise). Where the policy resets
     Ride the Bus after the Pair of 4s (2026-10-22, the fixture's race, 12 resets after each of
     the two Pair of 4s plays): 11 where the pace test turned the sparing play down, 1 at the
     finish; with the line played on as the pace plays it (above) 4-5 against the policy's line
     and 7 against the target, where the round is behind on the policy's play too: the target
     test is a cost budget (dropping it, `player_values`, 8,000 rounds a board, paired: Spare
     Trousers +0.0041 and +0.0051 of a round, the rest within noise, but a quick pass on
     `ride_the_bus_queens_dig` 3.3 → 7.6 s, and the fixture still picks a King play on seeds
     42-44), so the next lever is the futures' cost where the sparing line is at least the
     policy's, not another screen. Tried 2026-10-22 and
     not kept (PR #10): the pace test at `min(the policy's line, the target) × (1 − the growth
     added)` (a growth in won rounds taken as a share of points) with the finish's look-ahead
     measured over the winning play's whole growth: Spare Trousers +0.0030 and +0.0041,
     growers_4h3d −0.0012 ± 0.0006, seals_growers_4h3d −0.0015 ± 0.0003, the Square and
     seals_growers boards 3-4× slower, that quick pass 3.3 → 6.7 s, the fixture's pick moved on
     2 seeds of 3; the finish part alone growers −0.0019 and −0.0018 ± 0.0003 (a win's own
     growth is still there to win later: subtracting it shut the look-ahead to Green Joker's next
     +1); a chance-based growth (Lucky Cat's) isn't seen by the test, which scores every roll
     failing (the futures roll); the futures don't look ahead, so a growth a later play in the
     round would reset is weighed as the policy would reset it; with a win on the table the
     look-ahead tries one growth play, the one that grows most (then keeps the winning play's
     cards, then scores most), not every one, one step deep, and on 8 futures a loss rarer than
     1 in 8 can go unseen. So Best play counts a growth the rest of the round would keep, but not
     one it can't see kept: with Ride the Bus at +7 (raised in play 2026-10-15, fixture
     `ride_the_bus_queens_dig`, pending) it digs with a Pair of Queens, as every move it tried
     shows the reset; since 2026-10-17 Ride the Bus is also replaced there (below).
   - Since 2026-10-17 the projection grows the jokers a round changes as the simulated
     player's rounds leave them, not by a table (`value::Growths`, `sim::grown_over`): 16 runs
     of rounds played one after another, 3 an ante to Ante 8, at this ante's three plain blind
     sizes in turn (no boss's rule or size), each round dealt anew from your deck, the board
     carried from round to round (its round-end reset, and the round targets the game draws anew,
     `Board::draw_round_targets`: Castle's suit). A pool joker is measured on your board as it
     is, one ante (its growth label) and to Ante 8. A joker used up in at least half the runs
     doesn't last to Ante 8 (`LongRun::lasts`, as a perishable that runs out): yours leaves the
     projected board, its slot filled like any empty one, and is worth nothing to keep or lose
     there; a shop one is worth what taking nothing is (its slot the typical find an option is
     compared with). Obelisk, Lucky Cat, Yorick and Ride
     the Bus's resets are in the projection for the first time; `grow_antes` keeps what no round
     changes (Popcorn's round end, Hologram, Constellation, Madness, Campfire). Vampire isn't
     measured (`engine::GROWS_BY_TAKING_CARDS`): each round dealt from the same deck would let
     it strip the same card every round. What's left:
     - Since 2026-10-17 (the same day) your jokers' growth is measured again on runs played to
       the round's goals (`advise.rs` `played_for`, a second pass): the projection that keeps
       your jokers whose growth rounds measure however they project (`LongRun::played_for`:
       never let go, `fates`, else a joker the plain runs leave below a typical find would be worth
       nothing and nothing would be played for) gives what their numbers are worth
       (`round_worths`), each linear in its numbers without the floor at losing the joker, and
       used up as its numbers at none (`JokerWorth::floor`: with the floor every fall below a
       typical find was free and the second pass played as the first); the growth runs are
       played again on the same deals to goals made as Best play's (`LongRun::round_goals`: the
       jokers, Blue Seal planets, a dollar without what money held buys, a hand left), and the
       projection is built on what they leave, whether a joker lasts included. A joker that
       projection still replaces is dropped from the goals and the runs played once more, so the
       others aren't measured by a player that gives things up for a joker Best play counts as
       nothing (once, not to a fixed point). So a reset is
       priced by a player that avoids one where it's worth it. The shop's jokers keep the plain
       runs' growth (no goals of theirs are known: the same joker owned and in the shop can
       project apart). On `ride_the_bus_queens_dig` the goal-played runs leave Ride the Bus at
       +1.7 after 3 rounds and +1.1 by Ante 8, as the plain runs (+1.0, +1.1), still replaced (it
       would take about +19): on that flush board (Hanging Chad, Crafty, Droll) the plays that
       reset it are flushes with a face card, at a pace of about 9,000 against needs of 3,200 to
       6,400, beside non-face plays such as a lone Ace at a sixth of that, and the finish needs
       the face flush too; so the reset is the price of the board's flushes, not of the policy.
       (With goals of the jokers alone, a spare hand cost nothing and the runs left it at +5.3:
       a player that chases growth harder than Best play's.) Still circular: a joker the plain
       runs use up (`lasts`) is worth nothing in the projection that asks what it's worth kept,
       so the second pass never tries a line that keeps it (Ramen with fewer discards); unlike
       letting a joker go (`fates`), the keep doesn't reach it. No fixture's Best play
       moved; values move a little where a joker's growth is measured again (Spare Trousers
       0.0086 → 0.0082 a Mult on `goad_digs_gold_three`, Square 0.0036 → 0.0034 a Chip on
       `square_four_card_two_pair`; on `shop_keep_unfaded_ramen` Raised
       Fist ×1.22 → ×1.16 and the second and third options swap, Buffoon Pack over leaving, a 3%
       tier). Still, a difference now (one more Mult, ×Mult or Chip: `joker_worth`) is carried
       whole to Ante 8: a Ride the Bus or Obelisk reset is priced as lasting the run while its
       level is measured with the resets.
     - Since 2026-10-17 Best play's round also counts what the board it leaves does in this
       ante's blinds still ahead (`value::Ahead`), for every goal, as the founding question
       values a choice: its chance to get through times the long-run value of what it leaves.
       The blinds ahead (`Ctx::ahead`: the Big Blind's if it's still to come, taken as played,
       then the boss's) are played one after another on 300 runs (`sim::chained_runs`, by the
       player without goals, each round of each run dealt from its own stream so two boards
       stay paired past the first), each from the board the one before ends with, so a number
       the Big Blind would reset anyway isn't priced as lasting into the boss. Each goal's change
       of the chance to get through counts only where the paired runs show it either way
       (`compare::better_by`: the paired difference less 2 standard errors). Since 2026-10-18
       each change is the shift of the log-odds between the chances it was measured at, and
       every change is measured on your board carrying what the shops before each blind find
       (`Ahead::find`, since the second 2026-10-18 change: every score in a blind ×x, sized shop
       by shop on the boss alone from your board as it is, where the shop ranking's estimate is
       measured: the first shop's on every blind ahead, so the boss gets through as often as its
       chance plus one shop's lift with the money you hold, each next shop's on top from the
       blind after it, to that many shops' lift, the shop ranking's estimate of what money buys (on your board as it is, with no joker of yours to swap and, on a full board, one free slot it doesn't have: gap 17)
       (`money_value_with` as `survival_with` counts it; none in the quick pass,
       which screens no pool); then every blind raised alike where the board gets through these
       blinds less often than `AHEAD_FLOOR`, 10%); every shift is placed at that board's chance
       through these blinds, the reference, the chance it was measured at (no change of the
       board is moved from one chance to another, register); the shifts of what a round leaves are added and the chance they give taken
       against the reference (`RoundGoals::ahead`, `sim::chance_ratio`), so together they stay
       between none and a sure pass (several jokers' resets or growth against the joint runs,
       and five steps from one, in the register). With no shops counted and a board at 10% or
       more, one change alone is worth what its share was. A joker's numbers
       (only those its measured rounds move, `Growths::moved`, also now in `joker_worth`: a
       step on Ride the Bus's ×Mult made it a ×Mult joker, as the game reads it) are stepped up
       (the same steps), down a step of the same size and down to none (`sim::AheadWorth`: up,
       near, far, as a blind's chance isn't linear in them: a fall of a step may move nothing
       where a reset loses the boss; linear in the log-odds between the steps), and the joker
       taken away. The round's value keeps the two parts apart (`sim::Grown`, `RoundGoals::value`):
       1 plus the planets, money and card drawn by Ante 8, times the jokers' factors by Ante 8
       multiplied (each `ln(1 + worth)` added, the factor at least `LEAST_FACTOR`; not measured
       against the projection with several changed), times the chance with every shift of the blinds
       ahead against the reference; each part at least nothing, so a won round is never worth less
       than a lost one without a floor on their sum. An outcome from a board mid-round (the
       simulated player's finish, look-ahead and the oracle's futures) starts where the round
       so far left the jokers (`RoundGoals::at`, each against its own start), so the jokers are
       valued against the analysis start, and the player's thresholds are differences of the
       same value (`dollar_worth`, `planet_worth`, `grown_worth`). Losing a joker costs at least what its
       numbers at none do (its loss and its fall are gated apart). A Blue Seal's planet is a level more as the
       simulated round uses one (Constellation too), a shift of its own; a dollar is what $10
       more buys in the shops before each blind (`survival_with`), what it adds to the shops'
       lift, measured as the other changes are: your board carrying the shops' find for $10
       more against the find (since the third 2026-10-18 change, `Ahead::dollar`, register;
       with one blind ahead, where the find put the boss plus what $10 more adds, as a find
       sized to that gets through, with two measured on the runs, each blind's multiplier read
       off the points the find's own search tried), not gated by the paired runs (the runs
       size a change whose being there is the shops' estimate's, gated by its own rule of
       three, as the find itself isn't gated);
       a card worth
       drawing is your deck with the consumable on it against the same deck its Ante 8 worth is
       weighed against, as a share of a round added to that worth (`seen` is the best card
       drawn, one number). So a joker the projection
       replaces by Ante 8 still counts before then: on `ride_the_bus_queens_dig` (Ride the Bus
       at +7 before the Big Blind and The Pillar, whose own rule isn't modelled, gap 15) Best
       play discards Q♣ Q♦ J♥ 6♣ 3♥ to dig with the diamonds (89% this round against 98% for a
       play with a Queen), ahead of other lines that keep the Queens unplayed: the owner's
       line, the fixture's expectation restored (since 2026-10-18 Best play plays 10♦ 8♦ 6♣ 4♦
       there, tied with that discard; on seed 42 the search cut a better Queen-keeping discard by
       its budget, gap 1; since the find, below, Best play plays a Queen there again: with the
       shops' estimate lifting The Pillar alone from 69% to 99% the chain is 97%, Ride the Bus's
       +7 counts nothing in it, and what keeps the Queens out is The Pillar's own rule, gap 15,
       where the owner's line now waits). Since 2026-10-18 (the odds, the reference and the money above): 3 of
       139 saved picks moved, 1 BETTER (+2.5% ± 0.7%), 2 tie (one at 400 rounds −1.9% ± 1.4%,
       at 1,600 −0.1% ± 0.7%: two Spare Trousers, whose Ante 8 factors now multiply); the judge
       values with the same `RoundGoals::value`, so it partly judges this change itself; on the
       fixtures the jokers' figure moves a little where two jokers count or money and planets
       sit beside them (the figure is now what the jokers change of the round's value); the
       best-play step of a full in-blind pass the same within ±3% on four states. Since the
       find (2026-10-18, evidence in the register): on the fixtures only
       `ride_the_bus_queens_dig`'s pick moved (above; against the reference seeds 42 and 44 pick
       the best, seed 43 1.8% ± 0.7% below it, its best cut by the budget, gap 1), the others the
       same; 3 of 139 saved picks moved, all quick passes on boards below 10% (where the floor's
       find now lifts the board measured on, 3% → 9% and 0% → 12%, or a board at 8% is placed at
       its own chance): 1 BETTER (+1.9% ± 0.9% at 1,600 rounds), 1 tie, 1 unclear (+0.3% ± 0.4%
       at 1,600); the judge values with the same `RoundGoals::value`, so it partly judges this
       change itself; CPU within noise on 8 in-blind states, a quick pass −8% to +8% and a full
       pass −4% to +3% (the find's search: 4 sets of runs on the Ride the Bus fixture, 0.3 s). Since the money is measured as the other changes (the third 2026-10-18 change, register): no fixture's pick moved (`ride_the_bus_queens_dig`'s jokers' figure 0.0002 lower, two tied alternatives swapped in order); the 139 saved picks the same (the quick pass counts no shops, so no dollar there); CPU within noise on 8 saved in-blind states (a full pass −11% to +4%, Best play's step −21% to +9%, about what two runs of a state differ by): with one blind ahead the dollar takes no runs, with two one set through the blinds (and a search on the boss where the find's points don't reach). On the owner's saved states (2026-10-17) 6 of 136 picks moved (2 BETTER, 2 tie, 2 unclear within noise: −0.3% ± 1.0%, +1.8% ± 1.4%; the judge shares the valuation, so it partly judges this change itself); on the fixtures, a Big Blind with no grower (`big_blind_crafty_close_race`) keeps a spare hand a little more for the dollar's share in the boss (−0.7 points of this round on the same pick). A quick pass costs 0-6% more CPU on sampled in-blind states, 21-44% on the Ride the Bus states (the look-ahead now has a grower to play for). What's left: the next ante isn't
       counted (your board meets it after shops Best play doesn't model; on the board as it is
       it's often hopeless); a skipped Big Blind isn't weighed; the blinds ahead are played
       without goals, as the blind odds; between them the chain applies only the round-end
       reset and targets, not the game's other round-end steps (card.lua `end_of_round`:
       Popcorn, Turtle Bean, Gros Michel, a perishable's rounds, rent, the round's money), nor
       what a round does to your deck (Vampire's stripped cards are whole again), as the blind
       odds don't (gap 28); a card worth drawing is counted there only when its Ante 8 worth
       passed the screen (a budget); the shops' find is a multiplier on every score, the shape
       a ×Mult joker scored last has (outside Plasma Deck), not the joker the shops would offer
       (a +Chips stand-in measured within 0.039 of a round of it, a +Mult one placed last
       further, register); the shops' lift is the boss's (`money_value_with`), each shop's
       find sized on the boss, the first shop's standing for the Big Blind's own (its pool isn't
       measured: the Big Blind isn't among the rounds the pool is screened on); where the
       search's budget stops short of the estimate, the change is measured at the chance
       reached (the output says so); the money's change and the shops' lift read the shop's estimate,
       gated by its own rule of three (`measured_p`), the jokers', planets' and cards' the
       runs, gated by the paired test; what $10 more adds is the estimate's difference added
       to the boss's chance where the find put it (as the shop ranking's estimate of what money
       buys reads it), so where `AHEAD_FLOOR` raised a board the estimate leaves near
       none, $10 more adds the sliver it adds there, not the odds it multiplies there (no
       measure says which; the measure before, the odds, put 0.63 of a round on one board where
       the 1,200-run find said 0.075); with two blinds ahead the dollar is still off the 1,200-run
       find by 0.018 of a round on average, about as the measure before was (300 runs and where
       the find landed: the same noise every change measured there has); `AHEAD_FLOOR` still binds where the shops' estimate leaves a board near none (the estimate counts only the jokers money finds), now as the least chance the board the
       changes are measured on gets through these blinds; the money a round made before a
       mid-round outcome isn't carried into it (the same for every move compared there, it moves
       their order only where the chance ahead bends); a card worth drawing is a share added to
       its Ante 8 worth, not a shift, so it doesn't combine in the odds with the rest (with
       growth, the two can still pass a sure pass); the shop ranking's own survival is
       the boss's alone, from your board as it is (`rank_options`, gap 28); a joker's
       numbers to step are measured (`Growths::moved`), so an unmeasured one (Vampire) is
       stepped on all three; the growth runs' second pass (`played_for`) plays to Ante
       8 worths only, as its rounds are past these blinds.
     - Since 2026-10-22 the blinds ahead count the counter a joker's ability keeps that a round
       moves, apart from its three numbers (`Joker::counter`, game facts in the engine, checked
       by playing every joker: Loyalty Card's place in its cycle, from the hands played in the
       run; Yorick's discards to its next step; Seltzer's hands left): the chain started from the
       hands played as the analysis started, as if the round took no more, so where Loyalty
       Card's ×4 lands in the boss was never weighed, and a 4-hand boss loses it outright when it
       falls on a 5th or 6th hand. Each value the round in play can leave the counter at
       (`Joker::counter_after`, with its hands and discards left) is a shift of its own against
       the counter as it is (`Ahead::counter`, on the same runs as every change, gated by them),
       added to the joker's numbers' (`sim::CounterWorth`, read off the joker as each hand and
       discard leaves it, `RoundGoals::jokers_now`), so the hands a round takes are weighed: on a
       marginal board Best play spends a hand to land the ×4 on the boss's 4th hand rather than
       win now and leave it past the boss's last (`the_hands_a_round_takes_land_loyalty_cards_x_mult_in_the_boss`:
       0.41 against 0.09 there); one set of runs a value, at most `COUNTER_POINTS` (register).
       Measured against playing every simulated round's board on through the blinds ahead
       instead (each outcome with its own counters and more): that costs 2-4× the race's CPU and
       a 0/1 outcome a round, much noisier (register). Gemini's PR (#9, not merged) measured a
       spread of 0.000 on 34 of 35 saved Loyalty states and concluded it didn't matter; every one
       of those states gets through its boss whatever the phase, so the history can't tell, and
       on boards scaled to about half the phases moved the boss's chance by 0.35-0.80 (register).
       What's left: the counter's shift is measured from your board as it is (its numbers at the
       start, the chance there the reference), added to the numbers' in the odds, not measured
       with them (and floored at losing the joker where that loss was measured); the reference's
       counter is one no won round leaves (the round takes at least a hand: Loyalty Card one on,
       Seltzer one down), so every other change, the find and `AHEAD_FLOOR` are measured where
       no outcome lands, and each value of the counter is gated against it, not against the
       others (Loyalty Card one hand on against two, what a choice parts); the simulated
       player's look-ahead (`growth_play`) opens on what one play changes on the jokers, which
       winning now changes alike for a counter every hand moves, not on what playing on adds
       over the win it puts off: inside its rounds it looks where winning now already lands the
       phase and not where the hand more is what lands it, so such a line is found only at Best
       play's own move; the shops' find for the boss is sized on your board as it is, its counter as
       if the Big Blind took no hands either; nothing by Ante 8 (Loyalty Card's cycle turns
       many times by then and Seltzer runs out, `LongRun::lasts`, but Yorick's count is progress
       to its next ×Mult step, which its numbers give whole to the discard that crosses it:
       above; the fix values it as that progress through `joker_worth`'s worth of a ×Mult); the game's own counts (Supernova, Obelisk,
       above) aren't counters on a joker and aren't counted; Yorick's up to 23 values are read
       between `COUNTER_POINTS` measured ones (the nearest always among them), linearly, where
       its worth is a step (the blind-ahead discard its ×Mult lands on). (In an estimate, The
       Hook's discard steps Yorick's ×Mult but not its count, `JokerState`; The Hook is a boss,
       so no counter is counted there.)
     - The runs are on your board as it is at this ante's sizes: a later ante's board and
       target change how many hands a round takes.
   - The deck isn't counted: a card a play destroys or changes costs nothing in the round's
     value. Vampire is now one-sided (its ×Mult counts, the enhancements it strips don't: Best
     play feeds it Steel or Glass cards freely). A Glass card breaks 1 in 4 each hand it scores
     (card.lua `Card:shatter`; the engine takes it out for the rest of the round,
     `engine/score.rs`), so in a round won whatever is played, Best play plays it as readily
     as anything else (raised in play 2026-10-11, run I6XKKH65: Glass K♠ with Photograph and
     Hanging Chad, ×64 Mult as the first card scored, in a Small Blind every move won; the
     discard picked it, by $2 of money). Hand levels a round changes aren't counted either
     (The Arm's −1, Burnt Joker, Space Joker: `LongRun::planet` prices a level), nor the game's
     own counts a round moves (each hand's times played: Supernova's Mult, Obelisk's most played
     hand), nor by Ante 8 a counter on a joker (Yorick's discards: the discard that crosses gets
     the whole ×1, the others nothing, though each is a 23rd of a step; the blinds ahead count
     it, below). The fix: the engine reports a step's lasting card and level changes, valued
     through the deck projection (`deck_value`, `without_new_glass`) and `planet`.
   - What isn't modelled in a hand: what grows when a played card is destroyed (card.lua's
     `cards_destroyed` and `remove_playing_cards` contexts: Glass Joker, Caino).
   - Growth is weighed against win chance at the same unmeasured weight as money (gap 30).
19. **The card order tried is one proposal, checked, not a search.** The engine proposes the
   order its cards' maps say scores most (worked out in the order given; Mult only), and keeps
   it, or either half of it, only when the whole hand scores more as played (`engine::score`).
   Where an effect reads a card's place (Photograph: the first face card; Hanging Chad: the
   first card; Raised Fist: the last held card of the lowest rank) the maps were worked out
   for the order given, so the proposal can miss a better order (which face card leads, which
   card Hanging Chad doubles, which tied card takes Raised Fist), never claim one that
   doesn't score; a gain in chips alone (Hanging Chad on a Foil card) is never proposed; maps
   already in order propose nothing. An order that would earn less money or leave the jokers
   in another state (a Gold Seal or Lucky Cat on the card Hanging Chad doubles) isn't taken,
   though it might be worth more: the trade isn't weighed (it would need both outcomes for
   `RoundGoals`). The order is chosen with every roll failing, not on the rolls' odds. The
   general fix: for each effect that reads a place, each candidate for it (at most 5 played
   cards, or the tied held cards), the rest arranged by their maps with it fixed, all scored as
   played; and an order that changes money or the jokers handed to the caller's valuation.
20. **Big hands: moves from the structure, screened by their score now.** Since 2026-10-08 (D10)
   a hand of more than 12 cards gives Best play the plays whose cards can all score, over
   every card (`sim::scoring_plays`), each also topped up with the least-worth cards outside
   it, the best 8 of each kind and then by score now (`BIG_HAND_PLAYS`, 256 in all), and the
   discards of the hand's structure (`structure_discards`). Before, it saw only the first 12 cards by rank. Left
   open: the simulated player's best play still tries only the first 16 cards
   (`sim::SUBSETS_MAX`; the structure's plays over every card made the 32-card analysis's
   simulations ~10x slower: it needs a search that stops once no play left can beat the best
   found), a play outside the budget (one that scores less but keeps a better hand for later,
   pays money, or feeds a joker), a discard outside the structure's (a discard that pays money
   or feeds a joker: Mail-In Rebate, Faceless Joker, Castle, Hit the Road, Burnt Joker: a
   12-card hand races every discard, a 13-card one only the structure's), kickers beyond the 5
   least worth outside a play (with Splash every card scores and the best extra cards would be
   the highest), the simulated player's money discards (`decide_cards`: the first 12 cards),
   three orders of "junk" (`with_fillers`, `structure_discards`, `junk_order`), the first 10
   Stone cards and the first 64 cards. Known breach of rule 2: `structure_discards` (the
   oracle's keep-sets) isn't a generator over a defined class; the general fix generates
   discards as the complements of the structure's plays, plus the cards the engine's discard
   effects read, each class of identical cards counted once (which also lifts the Stone cap). The live page stops an analysis when the
   game writes another state (`progress`), shows how far it got, and runs at a lower CPU
   priority; a Juggle Tag's or The Manacle's hand size lasts only the round in play
   (`RunState::round_hand_size`). The motivating state (8 Juggle Tags, 32 cards; kept outside
   the repo: `~/.local/share/balatro-advisor/hand32-stuck.jkr`) took about two days, then
   ~15 minutes with the round's hand size, then ~3.5 minutes (quick; Best play ~170 s, its moves
   now from every card: it discards 5♣ 5♦ 4♣ 4♦ 2♠ where it threw K♥ Q♦ and the 10s).
   Still too slow to use in play (2026-10-11, run BLZMQ7NJ, The Eye at Ante 6 after Hieroglyph,
   5 Juggle Tags, 28 cards in hand): every quick pass of the round was stopped by the next
   save after 27–54 s (`timings.jsonl`: "stopped", 3–9 steps done), so the round was played
   with no Best play at all.
21. **Of the tags you hold only the Juggle Tag is read, and a tag's effect on a later blind is
   valued only in one case.** Skip-or-play values a Juggle Tag (+3 hand size, tag.lua
   `round_start_bonus`: the next round played) only when skipping the Big blind; on a Small
   blind skip it's "not valued". Since 2026-10-27 the Juggle Tags you hold give the next round
   played their hand size (`RoundStart::juggle`, set in `Board::at_blind_set`): the next blind's
   row, and the boss on skip-or-play's skip side of a Big Blind (whatever tag the skip takes; a
   Juggle Tag taken adds its own). Not read: a save in the moment between selecting a blind and
   its first cards drawn gives them to the blind after it. The other tags you hold (`save` reads them:
   `RunState::tags`) aren't used: a
   Double Tag copies the next tag taken (tag.lua), so with 7 Double Tags a Juggle Tag skip is
   8 Juggles (+24 hand size for the next round, with Steel cards held a large ×Mult), valued
   as one or none. The general fix: a tag's effect as a `Gain` for the round it acts on
   (through the one measure, gap 16), and held Double Tags counted on the tag a skip takes.
22. **A joker bought or sold during a blind doesn't change that blind's hand size or discards.**
   The game changes them at once (card.lua `add_to_deck` / `remove_from_deck`:
   `G.hand:change_size`, `ease_discard`; hands only from the next round, through
   `round_resets.hands`), but `advise::apply_mods` applies a joker's hand size, hands and
   discards only to rounds not yet started, so selling Juggler, Troubadour or Turtle Bean (or
   Stuntman, Merry Andy, Drunkard) mid-blind is valued as if the round in play kept them. Rare
   (most joker moves are in the shop). The general fix: `apply_mods` on the round in progress
   too, with hand size and discards, not hands.
23. **Shop options are ranked as alternatives, and a reroll doesn't count the shop it
   replaces.** `shop_options` values each option alone on your board as it is; the ranking
   says what to do first and is redone after each move. But a reroll redraws the shop's joker
   slots (packs and vouchers stay), and its option (`expected_best`) values its best find on
   the board without the jokers on screen, with nothing lost for throwing them away. Jokers
   that could all be bought are each valued alone, so two that work together (one Chips, one
   Mult) each look small, and the reroll ranks above buying them. Raised in play 2026-10-11:
   Ante 2, $34, 4 free slots, The Mouth with Wily Joker alone 6%, + Green Joker (rental, $1)
   10%, + Blue Joker (perishable, $5) 13%, both 38% (`whatif`); the ranking put the reroll
   first (its survival inflated by gap 17's sum), which throws both away, where buying both
   left $28 (interest kept) and the reroll still affordable after. A rental's sell value is its
   $1 price (card.lua `Card:set_cost`), so buying one into a free slot costs nothing to undo.
   Full slots hide it the same way: raised in play 2026-10-11 (Anaglyph Deck run I6XKKH65, Ante 2
   shop, 5/5 jokers, $8): Photograph (rental, $1) and Hanging Chad (eternal, $4) on screen.
   Photograph alone for Marble Joker took the Ante 3 boss from 56% to 113% of its target and
   ranked third, behind the Mega Buffoon Pack and the reroll; both for Chaos the Clown and Joker
   win that boss 92% of the time against 3% now and 64% with Photograph alone (`whatif`), and
   that move was never valued. The general fix: the moves tried include the shop's buys in
   sequence (the sets of jokers on screen that fit the slots, counting the sales that free them,
   and the money, then the reroll from the board they leave), each valued on the board it
   leaves, as the rest of the ranking is.
24. **A Stone card's rank and suit are shown and used, though the game hides them.** The game
   draws no front on a Stone card and its tooltip names only the enhancement (card.lua
   `Card:draw`: `ability.effect ~= 'Stone Card'`; common_events.lua `generate_card_UI`), but
   the save keeps its `base`, so `state` prints `7♥ [stone]` and the analysis can use it. In
   play the front does nothing: `Card:get_id` gives a random negative id (no rank: no pairs,
   straights, faces, Raised Fist) and `Card:is_suit` is false for every suit, flush or not, so
   a Stone held in hand breaks Blackboard whatever its hidden suit (the engine already does
   this: `engine/hand.rs` `is_suit`). Its seal and edition do act and are visible in the game:
   Red Seal retriggers and Purple Seal makes a tarot on discard (`Card:calculate_seal`), Gold
   Seal pays $3 when scored (`Card:get_p_dollars`), Blue Seal makes a planet at the end of the
   round (`Card:get_end_of_round_effect`); none of them checks for Stone. The front matters
   only when the Stone is taken off or copied: Vampire (`vampired`, then `get_id` gives the
   base rank), a tarot that sets another enhancement, Death, Cryptid, DNA; Strength and the
   suit tarots change the hidden front as they would a visible one. Where the advisor leaks
   it: the state and the page show the front (in Best play's cards and its "Arrange your
   hand" order too, where an instruction names a card you can't find), and code that
   counts suits or ranks over the
   hand and deck counts Stone cards' hidden fronts (`sim::keep_suit`'s commonest suit; to be
   audited elsewhere). What the player knows depends on how the Stone was made: The Tower on a
   card you chose, you saw (raised in play 2026-10-11: Anaglyph Deck run 2BTAA2XK, the 7♥ in
   hand turned Stone during the Ante 4 Spectral Pack, shown as `7♥ [stone]` at Ante 5); one
   Marble Joker adds is a random front over all 52 (card.lua: `pseudorandom_element(G.P_CARDS)`),
   never shown (the simulated rounds' own, since 2026-10-27 `Board::at_blind_set`, are all A♠: a
   fixed lean to Spades in whatever counts fronts, until this gap's fix), and so are Stones from
   Standard packs; a deck can hold both kinds. The save
   doesn't say which, and the advisor reads every front today, so with several Stones it
   "knows" the ones you never saw too. The history can tell them apart: a card keeps its
   `sort_id` through saves (card.lua `Card:save` / `Card:load`; `save.rs` doesn't read it yet),
   so a Stone whose `sort_id` the history saw as a plain card was seen by the player; one it
   never saw that way (made while the page wasn't running, or pruned) counts as unknown. This is
   the rule face-down cards already follow (178249c: shown as face-down, never planned around),
   which didn't cover Stone fronts. Knowing a Stone's front isn't knowing which Stone you
   drew: Stones that look the same (same seal and edition) can't be told apart on screen once
   shuffled, so the `sort_id` must not say which one is in hand. A Stone you drew is any of the
   look-alikes it could be (one known 7♥ and one unknown: half 7♥, half one of 52), and only a
   Stone that looks different from the rest keeps its own front. The general fix: a Stone's front is
   known (seen before it turned Stone) or unknown (one of 52, uniform); the display shows a
   Stone as what it could be ("Stone (was 7♥)" only when no look-alike could be it, else
   "Stone (?)" or "Stone (7♥ or ?)"), and the analysis never
   reads an unknown front: where it matters (the moves that take Stone off or copy it), it's
   valued over what it could be, as other random effects (gap 3). What it costs in play
   (2026-10-11, run I6XKKH65, Ante 4 Big Blind, Hanging Chad): Best play's order was "A♠
   Q♠ [stone] 10♣ 10♦ 9♥", the Stone first among the scoring cards so Chad retriggers it
   (+100 Chips, 8,280). The owner saw no Q♠, only a Stone, played the hand as it sat, Stone
   last, and the 10♣ got the retriggers: 4,600, and the round's chance fell from 99.8% to 84%
   on that one hand. So the display fix covers instructions too: a Stone is named as a Stone
   in every card list and order the page gives.
25. **A face-down card is never played, even when the hand's sort shows what it is.** Best play
   and the simulated player leave out every play that holds a face-down card (`sim.rs`
   `all_plays` and the play search's `hidden` mask; 178249c), so a face-down card is only ever
   held or discarded. Its stand-in is already bounded by what the sort gives away (`advise.rs`,
   4bb22a4: the game sorts face-down cards by their real key, cardarea.lua `CardArea:sort`,
   card.lua `Card:get_nominal`), but that bound only picks the stand-in; it never makes the card
   playable. Raised in play 2026-10-11 (Anaglyph Deck run 1RVL49LQ, The House at Ante 6, 3
   hands, no discards, 120,000 to beat): the hand sorted by suit read K♠ ? ? 6♠ 5♠ 4♠ 3♥ 5♦ 2♦,
   so both face-down cards are spades from 6 to K (a Stone sorts last, a Wild by its base suit
   and counts as a spade anyway), and K♠ ? ? 6♠ 5♠ is a Flush whatever they are: 43,000 at the
   least on this board, against Best play's Pair of 5s to dig (12,000). What's pinned
   depends on the sort: by suit it's the suit (and a rank range); by rank it's the rank when
   both neighbours share it, else a range. The general fix: a face-down card may be played,
   and a play that holds one is valued over what the card could be (the draw-pile cards that
   fit its bounds, a fresh pick for each simulated round, as other random effects: gap 3), not
   over the one stand-in picked today, so a play that relies only on what's pinned (the suit
   for a Flush) comes out sure and one that hopes for a rank is averaged. The shown score of
   such a play is a range. Fixture: `house_facedown_spades_flush` (its `best_hand: Flush` in
   `pending_expect`, `pending_gap: gap 25`: the replay test fails once it holds, so the fix
   moves it into `expect`; until 2026-10-14 it sat in a `pending/` folder the test didn't read).
   The same gap throws away a card the sort pins. Raised in play 2026-10-15 (Anaglyph Deck run
   Q84UC3NR, The Mark at Ante 4, 18,000 to beat): the hand sorted by suit read ? 9♠ 8♠ 4♠ ? 9♥
   2♥ ?, so the leftmost face-down card is a spade above 9 (and, as The Mark turns down only face
   cards, a J, Q or K of spades), four spades with 9♠ 8♠ 4♠. Best play throws all three
   face-down cards with 9♠ 8♠ (71% to win): a held face-down card can't be played later, so
   keeping it is worth nothing. The owner would keep the spade and throw the other two (not
   measured). The stand-in also misses what the boss says: under The Mark every face-down card
   is a face card, and the stand-in is any draw-pile card the sort allows. The general fix
   covers this: a face-down card is any card that fits everything you know about it, which
   means the sort and the boss's rule (The Mark: a face card).
26. **A joker added to a board goes in one fixed place, so the jokers' order isn't the one you'd
   pick.** Jokers act left to right (state_events.lua `evaluate_play`), so +Mult before ×Mult
   scores more, and you can reorder them for free at any time. `best_order` finds that order,
   but only for the board you own. The style outlook (`archetype_outlook`) puts a new joker
   into a free slot at the right end or into the slot of the joker it replaces, and never
   reorders the rest. The shop (`evaluate_candidate`) tries the freed slot and the right end,
   and leaves the others where they are. The long-run projection (`value::LongRun`) appends the
   option. Raised in play 2026-10-11 (run I6XKKH65, Ante 2 shop, outlook to the Ante 4 boss, 32
   rounds each): in the Face cards style, Smiley Face then Photograph reached 0.99 of the target
   and Photograph then Smiley Face 0.74. The outlook's pair is greedy over the top 3 singles,
   so a pair whose first joker lands in the wrong slot can drop out. That day Photograph +
   Hanging Chad reached 1.03, Smiley Face + Pareidolia reached 1.10 and was shown, and on 32
   rounds the two can't be told apart. The outlook also lists picks by their odds per shop, even
   when one is on screen now. The general fix: one step that places a joker added to a board
   (the order `best_order` would pick for the board it leaves, or the freed slot and the right
   end when the permutations are too many), used by every caller that adds one: the shop, the
   outlook, the long-run projection and packs.
27. **The long run can't see a joker's future partner, so nothing builds toward a combo.** The
   projection fills empty slots with stand-ins of three kinds (`value.rs` `fill_slots`: ×1.5,
   +15 Mult or +60 Chips, in the shares your shop pool offers), and an option is compared with a
   typical find (×1.25) in its slot. A stand-in never works with what you own, so a joker that
   is strong only with a partner (Photograph with Hanging Chad, Sock and Buskin or Pareidolia;
   Baron with Mime; Idol with Sock and Buskin) is worth the same by Ante 8 as if the partner
   could never show up, and a partner you'd buy later is worth only its stand-in's score.
   The style outlook (`archetype_outlook`) is a rough answer kept apart from the valuation: it
   picks pairs within hand-made style groups, greedily from the top 3 single jokers, on 32
   rounds (too few: two pairs 7% apart can't be told apart, gap 26's case), against a fixed
   Ante +2 boss, and lists picks by their odds per shop even when one is on screen. Raised in
   play 2026-10-11 (run I6XKKH65, Photograph + Hanging Chad in the shop, gaps 23 and 26). The
   general fix: the stand-ins drawn from your shop pool's real jokers (each at its odds of being
   found by the ante it would arrive), scored on the projected board with what you own, so a
   partner's chance counts in the long run, and an option that completes a combo is worth the
   combo. The outlook then becomes that search shown (which partners lift your board most, and
   their odds), with the groups only as labels.
   The deck has the same blind spot. Each change to it is valued alone, on the deck as it is
   (a consumable's targets, D8), so one Death, Strength or Hanged Man that moves the deck a
   step toward a shape (most cards within five ranks for Straights, one suit for Flushes)
   can't be told from no change and shows "no target", though the steps together are the
   point; and a Standard pack's cards count as nothing in the projection's spending (the
   register: "Standard counted as 0"), though for a Straight deck it's the easiest way to add
   the ranks the shape needs (easier than for Four of a Kind: any rank in the window helps).
   Raised in play 2026-10-11 (Ante 5, The Order, Devious Joker and Card Sharp, a Death in an
   Arcana pack: "no target" next to A♥ K♥ Q♥ J♥ and low cards in hand). The general fix, the
   same as for jokers: the projection draws the deck changes the run is likely to find (the
   tarots, spectrals and Standard pack cards in its pool, at their odds by the ante they'd
   arrive), so a step is worth what it adds to where the deck is heading.
28. **Every blind ahead starts from your board as it is now, not the one the blinds before it
   leave.** The blind odds, the next-ante horizon and the style outlook each simulate their
   round from the current board (`Board::at_blind_set`, through `Board::new_round`, only resets
   the round), so what a round does to the jokers is undone before the next: within a round the
   engine steps them (`Board::after_hand`), between rounds nothing does (nor what a blind's
   setup makes: Riff-raff's jokers and Marble Joker's Stone card, made again in each round
   ahead). Jokers that run out
   count in full in every round ahead (Seltzer's hands, Ice Cream's Chips, Popcorn's Mult a
   round, a perishable's rounds), and jokers that grow count at their size now (Green Joker,
   Ride the Bus, Runner: an under-count). Raised in play 2026-10-11 (run I6XKKH65, Ante 3 Big
   Blind, Seltzer with 3 hands left, used up by The Wheel): the Ante 4 boss showed 82% with
   Seltzer in every round and 11.5% without it (`whatif`), and Fortune Teller (perishable, 4
   rounds) is debuffed by then. The long-run projection doesn't have this problem for the jokers a
   round changes: it plays rounds one after another (`sim::grown_over`, gap 18), which is this
   fix's step for the jokers' numbers. Since 2026-10-17 Best play's round values what it
   leaves (the jokers' numbers, planets, money, cards worth drawing) in this ante's blinds
   still ahead played one after another (gap 18, `value::Ahead`, `sim::chained_runs`; since
   2026-10-18 measured on your board carrying what the shops before each blind find, sized to
   the shop ranking's own estimate, which stays the boss's alone and unchained): this fix's step for
   Best play, though the blind odds shown, the next-ante horizon and the
   outlook still start from your board as it is. The general fix: the rounds ahead chained, each starting from the board the simulated round before it ends with (its jokers'
   state, the perishables' rounds, its money), the same step the engine takes between hands.

29. **Plays that win the round now and are worth the same are shown as one arbitrary pick.**
   When several plays win on the spot and leave the same money, hands and planets, their
   values are equal, and the last tie-break is the cards themselves (`advise/play.rs`: `ck`,
   the canonical card order). The page shows that one as the play, not marked as a tie, so it
   can name a card that does nothing. Raised in play 2026-10-11 (run I6XKKH65, The Plant, 4
   Stones, a debuffed Steel J♥, 4♣, 8♣ and a Steel A♥ in hand): Best play was three Stones, the
   debuffed J♥ and the 4♣ (18,933), with four Stones and the 4♣ (22,943) and three Stones
   and the 4♣ (19,230) listed below it as if worse; any of them wins. Why these aren't
   flagged as ties isn't traced; the likely cause is the Noise stage: a tie is shown only after
   about 300 shared rounds (3 × the largest round value / n ≤ 1%), and before that the budget
   cuts moves of exactly equal value by the cards themselves, so equal plays can leave as
   "budget", not "equal". Not gap 1, which is which of several moves within 1% is picked
   (noise); here the values are exactly equal and the report hides it. The fix: plays that win now with equal value reported as
   one group ("any of these wins"), the highest score shown first, since it is the one that
   still wins if the estimate is off.


30. **Best play trades win chance for money by a dollar's weight that was never measured.** A
   won round is worth 1 + a dollar's weight × its money (`sim::RoundGoals::value`; since
   2026-10-14 a joker's growth is in the same sum, gap 18), the weight
   being what +$10 adds to the long-run projection, a tenth of it a dollar (`play.rs`
   `dollar_gain`, in the register as linear and unmeasured). A lost round is the run's end, so
   every point of win chance should cost a lot of money; when the weight is high, a few dollars
   outbid it. Raised in play 2026-10-11 (run I6XKKH65, Ante 5 Small Blind, a Lucky Gold Seal
   J♣ with Hanging Chad): Best play's discard won 97.2% with $10.35 of round money; another
   won 99.2% with $9.05. Picking the first means a dollar is worth at least 1.8% of a won
   round, +$10 at least 18% of the run's value, on a board the advice gives 5% at the next
   ante's boss. Likely cause, not traced: the projection values money as what it buys by Ante
   8, as if the run gets there (close to gap 13, which is the horizon past the win ante). The
   fix: the dollar's weight measured, from `calibration.jsonl` (what money turned into) or by
   valuing the money on the runs that survive, and checked on states where a move trades win
   chance for money.
31. **Best play says "use Death first" without naming its cards.** A move after a held
   consumable carries only its name (`PlayOption::use_first`); the cards it goes on are the
   ones its target search chose (gap 2: one target per consumable), shown only in the Tarots
   list's note. So Best play's cards are the hand after the use, and you can't tell which card
   changed. The fix: carry the target (`sim::Use::swap`, the cards before and after) to the
   option and say "use Death: 7♣ becomes 4♦ [steel]" in Best play, text and JSON. Raised in
   play 2026-10-12 (Anaglyph Deck, Ante 2 The Head: Best play said Death first, Tarots said
   7♣ → Steel 4♦).
32. **An open pack's "skip" wins ties, even when skipping keeps nothing.** A pick from a pack
   already paid for and "skip the pack" are ranked on the same measure (`rank_options`), and a
   planet for a side hand moves By Ante 8 by less than the noise can show (~1%), so every pick
   ties with the skip and the skip is listed first. Unless a joker grows from a pack skip (Red
   Card), skipping keeps nothing, and a hand level never lowers a score. D8's rule (the status
   quo is kept on a tie only when not acting keeps something) isn't applied to packs. Raised in
   play 2026-10-12 (Anaglyph Deck, Ante 5 after the Big Blind, Planet pack of Earth, Mars, Pluto,
   Saturn, Mercury with no Red Card: skip first, Mercury 1.007, Saturn 0.994, all tied). The fix:
   D8's rule for every race with a status quo, packs included: a skip that keeps nothing loses
   its ties to a pick (the best pick by paired estimate), and a skip that does keep something
   (Red Card's growth, a pick's money held) is measured, not assumed.
33. **Selling a Negative joker is counted as making room.** A Negative joker's slot goes with
   it (card.lua `Card:remove_from_deck`: the joker limit −1), so selling one to make room frees
   nothing. The sell choices (`advise.rs`, the "sell X for it" note) pick it like any other
   joker, so a pick or buy that needs a slot can be valued on a board the game doesn't allow,
   and the joker you'd really have to sell is never weighed. Raised in play 2026-10-12
   (Anaglyph Deck, Ante 4 Buffoon pack, 7/7 jokers: "pick Runner, sell Mad Joker (Negative,
   perishable, debuffed)" ranked first, By Ante 8 ×1.69; selling a real joker for it, simulated
   with `whatif`: Gluttonous Joker The Window 3% → 10%, Misprint, Gros Michel or Green Joker
   lower than not taking it). Not traced: how much of the ×1.69 is the slot, since Runner's own
   growth is projected from the Straights the simulated player makes on your board now
   (`value::Growths`).
   The fix: a Negative joker is never the joker sold to make room (it still can be sold for
   money, or because it's worth less than nothing), in every caller that makes room: shop,
   packs, rescue list, skip-or-play's Buffoon pack, `whatif`. Done 2026-10-24 for the advice's
   sell choices (`evaluate_candidate`, which the shop, packs, pool and Buffoon packs go
   through, the removal ranks the screen and the outlook read, and the projection's own swap
   and baselines: `LongRun::build`, `sell_base`); left: `whatif` sells what it's told. Since
   2026-10-25 a Negative joker can be let go by the projection (sold outright, its slot gone
   with it: `Fate::Sold`) and sold as a shop option of its own, judged by the shop ranking's
   measure; its slot also goes with it wherever it leaves the projected board (run out, used up),
   and losing one (`joker_worth`) is its slot gone, no find (no sale money: it's destroyed).
34. **The simulated player never banks a free hand before the hand that wins.** When a spare
   hand is worth almost nothing (money past the interest cap: `RoundGoals::dollar` ×
   `per_hand` near 0) and the hand you build toward falls a little short of the target alone, a
   strong player plays junk first: it digs like a discard without spending one, and its points
   let the built hand win alone as the last hand, which is what a Blue Seal's planet is made
   from (card.lua `Card:get_end_of_round_effect`: the last hand played). The policy plays junk
   only to dig once its discards are gone (`dig_for`), and to grow jokers at a finish
   (`growth_play`); a chase counts a junk hand as costing a hand, and on pace only a plan one card
   away that the best play would break is chased at all. So
   after a discard it plays the built hand first (short of the target) and finishes with
   whatever is left, and Best play prefers a first move that happens to bank the junk hand
   itself, even one that throws the cards the line is built on. Seen on fixture
   `head_keep_spades_and_seal` (run 99XSP7C7, Ante 6 The Head at $114, Spade Flush board,
   Blue Seal 3♠ in hand, 4 hands 2 discards, a Flush ~103k of 120k): since the player finishes
   by the round's value (2026-10-14, gap 18), Best play plays J♠ 9♥ 6♥ 4♠ as a junk hand,
   1.100 on the reference (seed 42) against 1.0755 for the owner's discard of K♣ 9♥ 6♥ 5♣
   (before: the discard led at 1.043); traces of the discard's rounds show the Flush played
   first, then a small hand to finish. The fixture's `expect` keeps only what still holds (the
   3♠ is never played or thrown); the owner's line (discard, never J♠ 7♠ 4♠ 3♠) waits in its
   `pending_expect`, and the replay test fails once that holds, so the fix restores it (D14).
   Not measured: that the owner's line really is better. Before 2026-10-14 it rested on the
   owner's judgement and on the older simulated player agreeing; "a strong player would bank a
   junk hand after the discard" is a guess, and so is any line picked by eye (the owner's firm
   objection was only to discarding the seal; keeping the Spades is the plausible line). So the
   work starts there: compare the owner's discard (K♣ 9♥ 6♥ 5♣) and the junk-play pick with the
   oracle playing the rest of the round (`Options::judge`, as the history check does: it finds
   its own lines), on the same rounds, by `RoundGoals::value`; the guessed line (discard, a junk
   hand of cards outside the Flush, then the Flush) is one more candidate, not the answer. If the
   discard wins, this gap is the cause and that margin is the fix's target; if it doesn't, the
   junk play may be right here (hands nearly free past the interest cap): take it to the owner
   before changing the fixture either way, never tune the policy until the old line wins. The
   fix: weigh a hand by what it's worth in the round (`RoundGoals`: a spare hand's money, points
   toward the target, a dig without a discard), not as a fixed cost, which is gap 6's general
   fix (a chase weighed by the round's value); a rule for "junk before a Flush" is the wrong
   fix. Done when the pending expectation holds (then restore it into `expect`), the reference
   ranks the discard first on seeds 42-44, and `player_values`, the player benchmark and the
   history check show no board or pick worse.
35. **Every face-down card is named "face-down card", so the page can't say which to throw.**
   `Card::label` names a face-down card by that one label (`model.rs`), in Best play, its
   alternatives and every card list. The analysis tells them apart, as each gets its own stand-in
   bounded by where it sorts (gap 25), and the JSON carries `indices`, but the page shows only the
   labels. With three in hand, "discard face-down card, 9♠" doesn't say which of the three.
   Re-sorting is not the problem: it re-analyses with face-down cards in hand (da6a41b), and the
   stand-ins don't depend on the current order. Raised in play 2026-10-15 (run Q84UC3NR, The
   Mark at Ante 4, the state in gap 25): Best play read "discard face-down card, face-down card,
   face-down card, 9♠, 8♠". That one throws all three, so it isn't ambiguous, but the owner
   couldn't tell from the list, and a move that throws one or two would be. Replay fixtures
   can't name one either (`best_not_cards` compares labels). The fix: name a face-down card by
   its place and what the sort shows ("face-down card, 1st from left: ♠ above 9"), in text and
   JSON, and give fixtures a way to name one by its place. It names the card, so it waits on
   nothing, but it matters most once gap 25 lets a face-down card be kept for a reason.
36. **Money past the cap buys nothing in the long run, and the race for the joker a buy
   replaces counts saves at the two bosses only.** Since 2026-10-24 the joker a buy replaces is
   chosen by the shop ranking's measure (`shop_measure`), every sale and place raced, ties
   reported, no joker kept out by name, a Negative joker never sold to make room (Moves tried;
   register "The joker a buy replaces"), and the projection's own swap and a sellable option's
   floor are judged with the money (`fates`: the joker's income and rent go with it, the find
   in its best slot; `floor_for`: scored on the board you'd hold). Since 2026-10-25 a joker's
   money is the round's as the game pays it, measured (register "A joker's money"): the money
   a round won pays with it against without it on the same rounds of this ante's plain blinds
   (`LongRun::money_board`: scoring, discards, the engine's round end with every joker's
   `calculate_dollar_bonus`, a hand left at `money_per_hand`), carried as money held every ante
   in the projection (`LongRun::income`: the board the projection holds against yours, and yours
   against it without the jokers the engine pays money for, so a copy counts once), so
   Business Card, Faceless Joker, To Do List, Rough Gem, Reserved Parking, Golden Ticket,
   Trading Card, Satellite and Delayed Gratification count, and a copy (Blueprint,
   Brainstorm) pays as the game pays it (discards and scoring yes, `calculate_dollar_bonus` no).
   The income table (`income_per_ante`) is retired; what a joker pays besides a round's play
   comes from engine facts (Rocket's raises, Egg's sell value, To the Moon's interest). Only the
   rounds both boards win count: a lost round ends the run, and what a joker does to the chance
   of winning is counted as that chance; a difference counts once clear of its own error. The
   play side of skip-or-play takes the round's measured money (`round_money_now`) for the old
   hand-set $1 of a hand left plus the table, so To the Moon's interest, counted there twice
   before (the save's interest amount and the table), is counted once. On 71 of the owner's saved full-slot shops with Business Card,
   Faceless Joker, To Do List, Rough Gem or Reserved Parking (owned or on offer), whatif on
   2,000 rounds: Business Card $0.4–7 a round, Reserved Parking ~$1.4, Faceless Joker $1–1.6;
   the joker a find replaces moved on none (the owned money jokers are still sold, rightly: run
   A7LD1T91's and B43IFPN9's money is past the cap, (a); 41EW553P's Business Card pays $0.36; on
   G1SF8AK5 selling Droll for a find halves The Manacle's odds, fixture
   `shop_faceless_sold_for_a_find`); every money joker on offer rose (Business Card ×0.56 →
   ×0.90 on PSU5PB8E's face-card board, fixture `shop_business_card_face_board`: whatif no odds
   lost and $1.4 a round won more after its rent, still below leaving the shop as an eternal
   rental holding a slot to Ante 8; Faceless ×0.78 → ×0.98 on GA74B7U3, Reserved Parking ×0.78 →
   ×0.87 on I6XKKH65, To Do List ×0.58 → ×0.64, Rough Gem ×0.99 → ×1.04). On the fixtures Best
   play moved on none; two shop orders moved (Grabber above the reroll on
   `shop_golden_kept_by_name`: its hand more pays $1 at a won round's cash-out, $3 an ante; a
   tie inside 1% swapped on `burglar_banner_no_discards`); 2 of 135 saved Best play picks moved,
   both unclear to the oracle (+0.01% ± 1.0%, +3.3% ± 11%; the judge shares this valuation).
   Player bench and parity unchanged. A shop analysis is 8–22% slower (a money board per sale and
   per joker bought). What's left: (a) money past
   the planet-level cap buys nothing in the long run (`LongRun::long_of`; `Spending`, what it
   buys in rerolls and packs, is built after the pool is valued), so a money joker's income is
   worth nothing to a rich run, kept or bought (run F67C37FV's Ante 3 shop at $60: Mail-In
   Rebate and Invisible Joker both worth nothing there, the swap between them decided by the
   rounds; the pool's finds sell Mail-In on `shop_next_ante_hopeless`, whose top option was
   pending on it until gap 17's walk restored it, 2026-10-26: a reroll's draw no longer counts on
   top of its money's). Tried 2026-10-27 and parked (the patch, not committed:
   `~/.local/share/balatro-advisor/patches/gap36a-money-past-the-cap-2026-10-27.patch`): where
   your money and $20 (`MONEY_REACH`) reach the cap, a first pass of the analysis at the screen's
   budget builds `Spending` from the pool and tarots, and the long run counts the money a
   board's jokers pay every ante past the cap through it (`Spending::factor_with`: paid before
   each shop of the money's walk, `Shops::income`), in the keep-or-let-go, the race, sales and
   buys. It works as built (F67C37FV's Ante 3 shop at $60: the projection keeps Mail-In, $9.57
   an ante past the cap ×1.29; a first pass costs about 8 s, 19 → 27 s) but exposed a bias it
   then multiplies: every buy there lost ×0.78 of its long run and the race kept selling
   Mail-In. It was read as the money measured on a board with a joker that changes how the
   simulated player digs falling (whatif: Greedy Joker cuts a round won +$4 → +$2, a plain
   Joker +$4 → +$5); on 2026-10-28 that row was the shop's Greedy Joker's rental ($3 rent,
   whatif's "− rent"): without it +$5, as the Joker, and its rounds pay as much (the Big Blind's
   money $4.7 against the Joker's $4.5), so the ×0.78 needs another cause; the simulated
   player's digs count their discards since (gap 8). Fixture `shop_mail_in_sold_rich` (selling Mail-In for
   $2 ranked above leaving) is pending on it, and on the money's walk counting the income of the
   board each option leaves (gap 17: its survival counts the sale's $2, not Mail-In's money
   before the boss). (b) The money is this
   ante's rate on your board as it is: Satellite's planets and Cloud 9's 9s as they are now,
   Matador's (a boss's rule triggers it; the plain blinds have none) and Gift Card's (every other
   card's sell value) not counted, and an option that sells a joker whose game role is money and
   whose money the engine doesn't pay says "not modelled" (`engine::PAYS_MONEY`: Gift Card,
   Credit Card, Matador; a label only); the empty slots' stand-ins earn no money while a joker
   bought into one earns its hands left; the rounds are played by the simulated player without
   the round's goals (`RoundGoals`, as `value::Growths`' first pass), so the money a goals player
   plays for (a paying discard cashed before the winning hand, discards kept for Delayed
   Gratification) is measured as the plain policy makes it; Green Deck's discards left pay in
   the measure, not in `RoundGoals::value`. (c) Mr. Bones' save counts
   in the race only at the two bosses it plays: a save on the rounds between them (this ante's
   Big Blind, the next ante's Small and Big) neither counts nor uses him up there. Since
   2026-10-27 the shop ranking counts it for every option through all of them (`save_lift`,
   register), a save is spent once for each copy (`Board::saves_lost_round`: the first that
   saves dissolves, state_events.lua `end_round`; the race and `swap_measure` leave a second
   copy for the next ante's boss; a Blueprint or Brainstorm copying him spends the same save; a
   Negative one takes its slot), and the ranking's trouble line counts it
   (`ShopOption::p_with_save`; the race's own trouble line reads the key round's save alone, as
   its rounds do). Not yet: the blinds ahead of Best play (`value::Ahead`) count chances on score
   alone, as the shops' lift added there is measured; a saved round pays no blind reward
   (state_events.lua `evaluate_round`) but the money's walk pays every round's; what the cash-out
   still pays a saved round (Gold cards held, the jokers' round-end money, Blue Seal planets)
   isn't paid in the simulated round;
   fixture `shop_bones_save_counted` (GA74B7U3's Ante 6 shop: Mr. Bones for Gros Michel read at
   The Goad on score alone, 29% against 87% with the save, ranked below Credit Card).
   (e) Invisible Joker's and Cartomancer's worth isn't modelled (gap
   16), so with no other cost to giving them up they go first. (f) A perishable on offer counts
   its income to Ante 8 in `long_of` (only Gros Michel's run-out is valued). Found reviewing PR
   #12, 2026-10-23; narrowed 2026-10-24 and 2026-10-25.
37. **The jokers' order is one order for the board, never chosen for the moment it acts in.**
   `best_order` picks one order over a fixed set of deals, and Best play scores the jokers as
   they sit. But the game asks the jokers to act at separate moments (`calculate_joker`'s
   contexts: a blind selected, each hand scored, each discard, the end of the round, leaving the
   shop), and a player rearranges them for free between any two. A copy joker (Blueprint,
   Brainstorm) copies whatever is to its right at that moment (card.lua, Blueprint's
   `calculate_joker` passes the context on), so the best order belongs to the moment: Blueprint
   beside Burglar when the blind is selected (+3 hands), then beside the joker that scores most
   for the hand played; beside Trading Card for a discard, Golden Joker or Rocket at the end of
   the round, Perkeo leaving the shop. Conditional jokers can make the best scoring order differ
   from hand to hand as well. The general fix: the order chosen per context as part of the move
   (Best play's plays and discards, the blind's start, the round's end), on an engine that models
   the copy in each context (the parity tier found `setting_blind` modelled only for Burglar
   itself, D17). Raised by the owner 2026-10-24, while the bot bench's joker order was fixed.
38. **The shop pool assumes every joker unlocked.** The game offers a locked joker in no shop or
   pack (common_events.lua `get_current_pool`: `unlocked ~= false`, legendaries aside), and
   `shop_pool` doesn't read the profile's unlocks (`meta.jkr`), so on a profile with jokers
   still locked the shops ahead are valued with jokers they can't show. On the owner's profile
   every joker is unlocked; on the bot's profile 35 weren't (13 of the 20 rares) until the
   owner unlocked all on 2026-10-23. The fix: the pool read with the profile's unlocks, as the
   parity tier already reads the profile for the game's own pools.
