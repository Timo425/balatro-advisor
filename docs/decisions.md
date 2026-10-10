# Decisions

Each entry: what was decided, why, and what would make us revisit it.
Status is `proposed` until the owner signs off at the end of a phase.

---

## D1. Implementation language: Rust (proposed)

**Decision.** Write the engine and CLI in Rust as one Cargo workspace. The CLI's
`--json` output is language-neutral, so other tools need no bindings. (An MCP server
was planned too and never built; see plan.md §6.)

**Why: the workload is a tight loop over many small evaluations.** A rough count
for one `shop` query, 5 jokers and 5 candidates with full slots:

| Factor | Count |
| --- | ---: |
| Joker configurations (baseline + 5 removals + 5×5 replacements) | ~31 |
| Monte Carlo samples per configuration (before early exit) | 500–2000 |
| Candidate plays per sampled 8-card hand (subsets of size 1–5) | up to 218 |
| **Scoring passes, before pruning** | **~3–13 million** |

Ordering is a different shape: up to 720 permutations × samples × the top few
plays per sample. That is about 1–3 million scoring passes once the best play per
sample is fixed.

| Option | Cost per scoring pass (rough) | 5 M passes | Notes |
| --- | ---: | ---: | --- |
| Pure Python | 15–50 µs | 75–250 s | Misses the 1 s target by 2 orders of magnitude |
| Python + NumPy | n/a | n/a | Branchy per-card, per-joker logic does not vectorise well |
| Python + Numba | 0.5–2 µs | 2.5–10 s | Heavy dependency; awkward with dynamic joker dispatch |
| Go 1.22 (installed) | 0.3–1 µs | 1.5–5 s single core | Good option; weaker type modelling for effects |
| **Rust** | **0.1–0.5 µs** | **0.5–2.5 s single core, <0.3 s on 12 cores** | `rayon` for parallel sampling, enums for effects |

The per-pass costs are estimates. The `bench` command, built in phase 3, will
replace them with measured numbers. Pruning (below) cuts the pass count by
another 5–20×, so Rust has headroom and Go would probably also pass. Rust wins
on three points:

- Scoring works on floats (`f64`, the same doubles as Lua). Exhaustive `enum`
  matching makes a joker that silently falls through a case a compile error,
  not a wrong number.
- `rayon` gives data-parallel Monte Carlo with no extra design.
- The two most relevant reference implementations, balatro-rs and its `.jkr`
  codec, are Rust. Their structure and tests are easier to learn from in the
  same language, even though we are not depending on them (D2).

**Toolchain issue.** The installed `rustc`/`cargo` is 1.75 (Ubuntu apt, Dec
2023). The official Rust MCP SDK `rmcp` 3.5 needs Rust **1.88** (edition 2024),
and much of the current crate ecosystem has moved past 1.75. Options:

1. **Install `rustup` (user-local in `~/.rustup` and `~/.cargo`, no sudo, about
   1 GB)** and pin the toolchain with `rust-toolchain.toml`. *Recommended.*
2. Stay on 1.75 and pin older crate versions. The MCP server would then be a
   hand-written JSON-RPC-over-stdio loop, roughly 300 lines. This works, but it
   means maintaining a protocol implementation ourselves.

→ **Needs owner approval** (this is a heavy toolchain addition).

**Revisit if** another tool needs in-process calls on a hot path. In that case,
add a binding crate (e.g. `pyo3`) over the same library and keep the engine as is.

---

## D2. External engines: reference only, no dependency (proposed)

### evanofslack/balatro-rs

- **License.** Every crate's `Cargo.toml` says `license = "MIT"`, but the repo
  has **no LICENSE file**. Copying code needs the MIT notice text; without the
  file we would be reconstructing it. That is acceptable, but it is a smell.
- **Activity.** Active: last push 2026-09-27, 14 stars, not a fork. That is
  also fast churn for a git-only dependency.
- **Coverage.** 74/150 jokers (README). No skip tags, alternative decks or
  alternative stakes.
- **Fidelity problems for our use**, read from `core/src/game.rs::calc_score_inner`:
  - chips and mult are `usize`. Polychrome is `mult += mult / 2` and Steel held
    is the same, so ×1.5 truncates (7 mult → 10, not 10.5). The game uses Lua
    doubles, and late-game scores depend on those fractions compounding.
  - Stone cards are scored in a **separate pass after** the other scoring cards.
    In the game, `evaluate_play` inserts them into `scoring_hand` and then
    **sorts by screen position** (`table.sort(scoring_hand, … a.T.x < b.T.x)`),
    so a Stone card sits in play order among the others. That matters when Glass
    cards or per-card ×Mult are to its left or right.
  - Scoring mutates game state (`planetarium.play`, The Arm `level_down`) inside
    the score function. We need a pure function that we can call millions of
    times on hypothetical states.
- **What is genuinely valuable:** the `.jkr` codec's edge-case fixture list
  (escaped strings, numeric keys, negative/float numbers, `inf`/`nan`), and
  `balatro-seed`, a byte-accurate port of the game RNG. Seed-accurate
  shop/pack prediction is out of scope, but it is the natural dependency if
  that ever comes into scope.
- **Decision:** reference only. Our `.jkr` parser is about 150 lines, and we
  test it against the same kinds of edge cases (written by us, not copied). If
  code is ever copied, it gets an attribution comment plus the MIT notice in
  `THIRD_PARTY.md`.

### taggarttufte/balatro-rl

- **License.** **None** (no LICENSE file and no license field). By default that
  means all rights reserved: we **cannot copy or vendor** anything from it.
- **Activity.** Archived (2026-08-26).
- **Coverage.** About 200 joker classes across `balatro_sim/jokers/*` (a class
  count, not verified as distinct jokers), plus a 496-test suite.
- **Fidelity problems:** the scoring model is
  `score = (base_chips + chips) × (base_mult + mult) × mult_mult`. That applies
  every ×Mult to the **final** additive mult, which is wrong for Balatro, where
  ×Mult multiplies the running mult at the moment it triggers. So "+Mult
  before ×Mult" order effects, the thing we most need to get right, cannot be
  represented. It also has Lucky at 1 in 4 (the game uses 1 in 5), and Steel
  applied as a played-card effect.
- **Decision:** reference only, for ideas about joker grouping. No code.

### The game itself

`Balatro.exe` is a zip containing the Lua source. It is the ground truth for
evaluation order, joker configs and save layout. It is proprietary, so:

- we **read it locally** (`unzip -p Balatro.exe functions/state_events.lua`) and
  cite file and function names in comments and docs;
- we **never commit** its source (`/game-src/` is gitignored);
- joker facts (name, rarity, cost, numeric config values) are functional data
  that is also published on the wiki. A dev script extracts them from the
  owner's local install into `data/game.json` (planned as `jokers.base.json`),
  and we commit that generated file (values only, no code). A test re-checks it against the
  install when one is present.

### Where ideas came from: `balatro-agent`

`balatro-agent` is the owner's separate, local Balatro bot project. Its
`tools/balatro_state.py` gave us the Lua-table parser idea, the blind targets per
stake and the "what the save hides" notes, copied and adapted into Rust with an
attribution comment. Its effect-text scoring was not reused (it estimates rather
than models the game). Nothing is imported from it, and the advisor works without it.

---

## D3. Numbers: `f64` everywhere, like the game (proposed)

Lua numbers are doubles. In vanilla, `mod_mult` is the identity and
`mod_chips` only clamps for one challenge modifier (`chips_dollar_cap`).
Nothing floors during the pass: the round total adds
`math.floor(hand_chips*mult)` (`state_events.lua`, end of `evaluate_play`). We
mirror that: `f64` through the whole pass, and `floor` once at the end.
`naneinf` is reported as such, not as an overflow panic.

---

## D4. Docs and code language: English (proposed)

The spec is in English, and agents and scripts read the docs and JSON. User-facing CLI
text is also English. Easy to flip if the owner prefers Estonian comments.

## D5. How the advice is built and changed: four stages, one measure, a required workflow (accepted)

**Decision.** Every piece of advice goes through four stages (moves tried, simulated player,
valuation, noise) with one measure everywhere (winning, times the long-run value of what a
choice leaves you), described in `docs/design.md`. Every change to how advice is decided or
valued follows the workflow in `CLAUDE.md`: replay fixture, diagnosis, a fix to the stage that
failed, proof (tests, replay, snapshots), review by the `advisor-reviewer` agent (and again
on its fixes), and a commit named after the stage. The rules themselves change only through
a decision entry here, and only by widening or replacing a rule, never by an exception.

**Why.** Fixing each wrong suggestion with a rule for that case (a Cryptid tip, a "keep Blue
Seals" rule, a spade-dig template) made the same kinds of mistakes come back, and earlier
fixes resurfaced. Fixing the stage that failed improves every similar situation at once, and
a reviewer that hasn't seen the reasoning finds what the author missed.

**Revisit if** the stages stop fitting how the advice works, or the workflow costs more than
the mistakes it prevents.

## D6. Push by default; the phase rule retired (accepted)

**Decision.** Every change is committed and pushed once its checks passed: for a change to
how the advice decides or values things, the workflow's step 4 (tests and replay exit 0,
snapshots explained) and the `advisor-reviewer` run on the fixes (`CLAUDE.md`, D5). Asking
first is kept only for heavy dependencies and scope changes. The "Phased work" rule is dropped
and `docs/plan.md` §9 is kept as history.

**Why.** "Ask before pushing" contradicted the workflow's "commit and push", and asking added
nothing: the owner doesn't review this repo's docs or instruction files before they go out,
and the proof and the independent review already gate changes to the advice. The phases
(2–4 shipped) no longer describe the work, and what their stops were for (owner review
before the scope grows) is covered by "ask before changing scope". (First accepted as a
scoped exception for the workflow's push; widened the same day.)

**Revisit if** a pushed change turns out to need the owner's look before it's public, or the
work goes back to planned phases.

## D7. Changes to the search are measured against a reference (accepted)

**Decision.** A change to Best play's search or the noise stage is proven by
`search_against_reference` (tests/replay.rs): every move it considered, played on
`compare::MAX` rounds the search didn't use, and how close each fixture's pick is to the best;
run before and after, and the change judged by that, not by which fixture flips (CLAUDE.md step 4).

**Why.** Several fixtures sit within the 1% tie margin of other moves, so any change to how the
search cuts or ties options shuffled which one came first, and fixing one fixture broke another
(2026-10-03: widening the budget and an optimistic cut each fixed one fixture and broke another).
The reference says which picks are really worse (big_blind's junk play: 1.2% below the best) and
which are ties, so the search can be fixed where it's wrong instead of tuned to the fixtures.

**Revisit if** the reference itself becomes too slow to run on every search change, or the
fixtures stop covering the kinds of states the search gets wrong.

## D8. A change the search can't tell from no change isn't made, when not acting keeps something (accepted)

**Decision.** In a race with a status quo (every consumable target search: "no target", your
deck as it is), the status quo is never cut by the budget, only by a finding (clearly worse,
or shown equal). When it ends among the options as good as the leader (shown equal, or still
undecided at the cap), it's kept, so the change isn't made, if not acting keeps something for
later (a consumable stays held). This is the use-now decision on the hand on screen: valued on
sampled hands, where nothing is kept for, the best set is taken. A chance that's gone if unused
(DNA's copy each round) takes the leader, as before. `compare::race_keeping`, `target_race_priced` (`UsePrice::keeps`).

**Why.** Every target search took its leader, so a set that led on noise was used: DNA copied
a plain card now and then, a pack card could be credited with a consumable on it, and the
budget could drop "no target" before any finding, which the noise stage says a budget cut must
not stand for. Doing nothing keeps the consumable for a later, better target, so on a tie
it's the right default; the same rule already decides the spare money split (levels unless
events are clearly better). Where not acting keeps nothing, the tie rule withheld real gains:
DNA, priced for the hand a copy spends, copied a Steel King in only 7 of 13 rounds holding one
(one card's copy is a small change that 128 rounds often can't separate from none), so DNA was
undervalued; 256 rounds didn't fix it and cost ~1 s an analysis. It widens the noise rule ("what can't be told
apart is never ranked by noise") to the choice between acting and not acting.

**Revisit if** it withholds changes that later prove clearly better (the reference measurement
or the replay fixtures show a target search keeping the status quo against a clear gain), or
its cost (about a third more samples per target race) becomes a problem.

Proposed by the cloud session in PR #2 (as `race_keeping`, 391cd2e, reverted pending approval);
approved by the owner 2026-10-04, then narrowed to uses where not acting keeps something (owner,
the same day).

## D9. Simulated player changes are judged by the player benchmark (accepted)

**Decision.** A change to the simulated player (`sim::decide` and what it calls) runs
`tests/player.rs` before and after: win rates on one synthetic board per play style at fixed
targets (the 50% points of the player at 3cfb122), and the gap to the oracle (`sim::set_oracle`:
a slow player that tries alternatives on simulated futures, never used by the advice). Every
board that gets worse is explained. CLAUDE.md step 4.

**Why.** The replay fixtures are one run's board (Spades, 4 hands, 2 discards), so they can't
show how the player handles other play styles, and the reference measurement (D7) judges the
search against the same player. The rulebook only chased flushes and lost 5 to 33 points
against the oracle on boards it didn't fit (straights, trips, Misprint, held-card jokers)
without any fixture noticing.

**Revisit if** the synthetic boards stop resembling real runs (calibration of win chances
disagrees with them), or the fixed targets drift so far that every board is won or lost.

Approved by the owner 2026-10-04.

**Widened 2026-10-09 (owner's go-ahead).** Five boards with what the first twelve lack and real
runs have: jokers that grow during the round (Spare Trousers, Green Joker, Ride the Bus, Runner),
a deck with a few strong cards (Bonus, Glass, Steel, Mult, seals) and debuffing bosses (The Goad,
The Club), at the half points of the player at d4195a0. Found by a regression they didn't see:
after 7374df9 (the round plays on the board each play leaves) the pace rule took a grown Spare
Trousers pair as on pace and Best play threw a Bonus Gold Seal 3 (gap 6). The win rates alone
wouldn't have flagged it: at 7374df9 they rose from 8-27% to 45-54% on the grower boards, since
before it the simulation didn't grow those jokers at all. A change to the simulated game moves
win rates whatever the player does; the gap to the oracle is what judges the player: +9.8 ± 1.4
(trousers, 3 discards), +13.3 ± 1.4 (trousers_goad, 3 discards) and +14.4 ± 1.4 on the Goad
state's fresh round, against +1.6 to +5.8 on the others.

## D10. Moves tried: every move while the search can take them; a bigger hand's from its structure (accepted)

**Decision.** "Moves tried: all of them" is replaced by: every move while a hand gives no
more than a race can take (every play and every discard of up to `sim::ALL_MOVES_MAX` = 12
cards, as before); a bigger hand's moves come from its structure, over every card, with no cut
by the cards' order (`sim::big_hand_moves`): the plays whose cards can all score
(`sim::scoring_plays`, a superset checked against every subset), each also topped up with
the lowest cards outside it (kickers, or junk played to dig), the best 8 of each kind (its
hand and how many cards it plays) and then the best by score now, `BIG_HAND_PLAYS` (256) in
all, a budget; and the discards the hand's structure gives (the oracle's:
`structure_discards`, keeping the best of those plays first). The output says when this
applies (`heuristics`). Rule 2 and "Search, then narrow" in design.md are widened to say
so: where candidates are more than a race can take, a generator over every card by the
game's rules (a superset of a defined class, checked against every candidate on small
cases), and any cut beyond it a budget named in the register and the output. The simulated player's best play keeps its cut (every subset of the
first 16 cards by rank): over every card it cost ~10x on a 32-card hand, where it's chosen
hundreds of thousands of times an analysis (gap 20).

**Why.** Raised in play (gap 20): 8 Juggle Tags gave a 32-card hand, which has 242,824 plays
and as many discards, each needing simulated rounds, so "every move" can't be run; the
code had quietly taken the first 12 cards by rank instead, which can hide the plays that
matter (five 8s, a flush low in rank), and the analysis still took ~14 minutes. The owner
asked for the tool to be reasonably fast and reasonably accurate rather than exhaustive in
name only. Most big-hand moves are the same move with other junk: the structure gives the
plays that can score, the budget keeps the best few of each kind so it isn't all one hand,
and the noise stage then decides as usual.

**Revisit if** a big hand's pick is shown worse than a move this leaves out (a play outside
the best `BIG_HAND_PLAYS` by score now, or a discard outside the structure's), or hands over
12 cards become common enough for the budget to matter.

Approved by the owner 2026-10-08.

## D11. Review: the Opus reviewer, plus narrow Haiku checks on changes that touch many places (accepted)

**Decision.** Step 5 of the workflow keeps one `advisor-reviewer` on the model the session
runs (Opus), open-ended over the whole change, and once more on the fixes. A change to how
advice is decided or valued that touches many places (several stages or files, callers to
follow, docs and tests to keep true) also gets 4–8 Haiku checkers next to it, the same agent
with `model: haiku`, effort high. Each gets ONE bounded check written for the change and the
files it may read, split by code area: e.g. game fidelity against the Lua, every caller or
board the change must reach, determinism and paired draws, edge cases, tests that would fail
without the change, docs and labels still true. Every finding gives file:line, a failing
scenario, severity, confidence and the evidence read, plus what was checked and found fine;
every one is verified in the code before acting. A small change gets the Opus reviewer alone.
The template is the owner's "Detailed review" prompt (Obsidian, `AI/Agentic/Subagentide
fan-out (kokkuvõte ja review).md`).

**Why.** Two trials on real changes. Round 1 (2026-10-08, card arrangement): five Haikus on
broad angles over the whole diff, three of them raising the same confident, impossible bug;
not worth it. Round 2 (2026-10-09, The Hook in the simulated round): eight Haikus with one
narrow check each raised no false alarm of that kind and found six things the Opus review
didn't (whatif's next-ante boss keeping the blind in progress, The Tooth mislabelled, the
round loop and Burnt Joker's skip untested through the simulation, the pick depending on card
order, a doc name), at about 4.8x the Opus review's tokens, which cost far less each. The Opus
reviews found what mattered most both times (round 2: the lost "not modelled" label, the
missing proof in a Hook state, the general fix, and on the second pass the pick matched by
position), so they stay; the Haikus are a cheap addition for the mechanical checks. The
orchestration (writing the checks, verifying each finding: ~30 in round 2) is on the session's
model, which is why small changes skip them.

**Revisit if** the Haiku checks stop finding things the Opus review misses, their false
findings cost more verification than their finds save, or a cheaper or stronger reviewer
model changes the trade.

Approved by the owner 2026-10-09.

## D12. Proof in two tiers; picks a change moves on the owner's history, judged by the oracle (accepted)

**Decision.** Every change to how advice is decided or valued runs a fixed set of checks:
tests, clippy, the snapshot diff, and the history check (`tests/history_picks.rs`, which
uses only long-stable API so it runs on older base commits, and `tests/history.rs`): Best play's quick
pass on the states the live page saved in a blind (`~/.local/share/balatro-advisor/history`,
one in three, chosen by name so the sample holds as the history grows) on the code before and
after, and every pick that moved (by the cards' places in the hand, so a new arrangement isn't
a move) played against the old one on the same rounds with the oracle making the policy's
decisions (`advise::Options::judge`, `sim::set_oracle`). Verdicts use the search's tie margin:
BETTER or WORSE only when clearly apart and by more than 1%, tie when within it both ways,
unclear otherwise; a pick that can't be judged (?: a consumable's old target can't be rebuilt)
or that the change lost (LOST) is listed, not dropped; every WORSE, unclear, ? and LOST pick is
looked at and explained or fixed. The slow
checks run where they apply: the full `search_against_reference` for a change to the search or
the noise stage, else on the fixtures whose Best play moved; the player benchmark (win rates
and the gap to the oracle, D9) for a change to the simulated player or the simulated game.
CLAUDE.md step 4.

**Why.** The Goad regression (gap 6). 7374df9 made the simulated round grow Spare Trousers,
which pushed the pace rule into playing a lone Bonus pair, and Best play threw a Bonus Gold
Seal 3. No fixture held such a state, the player benchmark had no such board (its win rates on
the boards added since rose: D9), and every other check judged moves with the same simulated
player, so it agreed with the flaw; the owner found it in play. The history holds the
situations the owner actually meets (353 states in a blind over 11 runs by 2026-10-09), so a
change that moves picks there shows without waiting for the owner. The oracle narrows what the
policy gets wrong: on the Goad state the policy wins less holding the Bonus 3 than a plain one
(90.8% vs 93.3%, impossible for a correct player), the oracle doesn't (93.7% vs 93.4%).
Running the slow checks only where they apply keeps a fix near an hour instead of four.

First run, on 7374df9 itself (128 sampled states, its parent's picks against its own, judged
by today's code, verdicts then by 2 standard errors without the tie band): 9 picks moved, 6
judged better, 3 the same, none worse. The Goad pick (not in the sample) is judged unclear
(the new pick +1.0 ± 0.8%): the check shows it as moved and worth a look, not as worse. What
the pace flaw costs shows in the oracle gap on the Trousers boards (+9.8 and +13.3 points,
D9), where the pace fix is judged.

**Limits.** The judge is independent of the search, the noise stage and the policy's
decisions while no win is on the table. It is not independent of the valuation (both moves are
measured by the code's `RoundGoals`), the engine, or the finish and consumable use (decided as
the code does when a win is on the table), and the oracle's own look-ahead plays on by the
policy, so it narrows a policy flaw but needn't remove it. A change to those is half judging
itself, and its verdicts say so. Shop choices, value across antes and the engine's fidelity to
the game still rest on the owner and the calibration log. The quick pass takes 1-60 s a state
by the Ante (median 2.6 s; all 353 in 58 min), hence the sample (about 20 minutes each side;
the judge about 30 s a moved pick).

**Revisit if** most moved picks are ties flipping rather than changes, the oracle's verdicts
disagree with the owner's on cases the owner can explain, the sample misses regressions the
owner then finds in the states left out, or a stronger independent judge (a deeper search,
real outcomes) becomes cheap enough.

Approved by the owner 2026-10-09.
