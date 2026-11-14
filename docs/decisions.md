# Decisions

Each entry: what was decided, why, and what would make us revisit it. Status is `proposed`
until approved, then `accepted`: by the owner for scope, dependencies, what the advice values
and the workflow; by the reviewer for a technical rule (`CLAUDE.md`, "When the rules don't fit
the fix"). What needs an entry: `CLAUDE.md`, "What needs a
decision" (a change that makes a quoted sentence of the design or the workflow untrue). A change
to a decision amends its entry in place (marked amended, with the date), not a new entry on top.

---

## D1. Implementation language: Rust (accepted)

**Decision.** Write the engine and CLI in Rust as one Cargo workspace. The CLI's
`--json` output is language-neutral, so other tools need no bindings. (An MCP server
was planned too and dropped: an agent that can run commands gets the same from
`analyze --json` and `whatif --json`.)

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

The per-pass costs are estimates; the `bench` command measures them. Pruning (below) cuts the pass count by
another 5–20×, so Rust has headroom and Go would probably also pass. Rust wins
on three points:

- Scoring works on floats (`f64`, the same doubles as Lua). Exhaustive `enum`
  matching makes a joker that silently falls through a case a compile error,
  not a wrong number.
- `rayon` gives data-parallel Monte Carlo with no extra design.
- The two most relevant reference implementations, balatro-rs and its `.jkr`
  codec, are Rust. Their structure and tests are easier to learn from in the
  same language, even though we are not depending on them (D2).

**Toolchain.** Rust from a user-local `rustup` (`~/.cargo`); the apt 1.75 was too old for
much of the crate ecosystem.

**Revisit if** another tool needs in-process calls on a hot path. In that case,
add a binding crate (e.g. `pyo3`) over the same library and keep the engine as is.

---

## D2. External engines: reference only, no dependency (accepted)

**Decision.** No external engine is a dependency; they are read for ideas only, and anything
ever copied gets an attribution comment (plus the MIT notice in `THIRD_PARTY.md` for MIT code).

**Why.** Surveyed at the start: [evanofslack/balatro-rs](https://github.com/evanofslack/balatro-rs)
(MIT in `Cargo.toml` but no LICENSE file; 74/150 jokers) scores with `usize` chips and Mult, so
×1.5 truncates, scores Stone cards in a separate pass instead of in play order
(`evaluate_play` sorts `scoring_hand` by screen position), and mutates game state inside the
score function; its `.jkr` codec's edge cases and `balatro-seed` (a byte-accurate port of the
game RNG, the natural dependency if seed-accurate shop prediction ever comes into scope) are the
useful parts. [taggarttufte/balatro-rl](https://github.com/taggarttufte/balatro-rl) (no
license, so nothing can be copied; archived) applies every ×Mult to the final additive Mult,
which can't represent order effects, and has Lucky at 1 in 4. The ground truth is the game
itself: `Balatro.exe` is a zip of its Lua source, read locally and cited by file and function,
never committed (`/game-src/` is gitignored); joker facts (numeric config values, also on the
wiki) are extracted from the owner's install into the committed `data/game.json`, re-checked
against the install when present. [`balatro-agent`](https://github.com/Timo425/balatro-agent),
the owner's separate bot, gave the Lua-table parser idea, blind targets per stake and "what the
save hides" notes, adapted into Rust with attribution; nothing is imported from it.

**Revisit if** an engine appears that scores in game order with doubles and has a license we
can depend on.

---

## D3. Numbers: `f64` everywhere, like the game (accepted)

Lua numbers are doubles. In vanilla, `mod_mult` is the identity and
`mod_chips` only clamps for one challenge modifier (`chips_dollar_cap`).
Nothing floors during the pass: the round total adds
`math.floor(hand_chips*mult)` (`state_events.lua`, end of `evaluate_play`). We
mirror that: `f64` through the whole pass, and `floor` once at the end.
`naneinf` is reported as such, not as an overflow panic.

---

## D4. Docs and code language: English (accepted)

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
first is kept only for heavy dependencies and scope changes. The "Phased work" rule and the
original plan's phase table are dropped.

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

## D12. Proof in two tiers; picks a change moves on the owner's history, judged by the oracle (accepted, amended 2026-10-23, 2026-11-03, 2026-11-14)

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
itself, and its verdicts say so. Shop choices and value across antes still rest on the owner
and the calibration log; the engine's fidelity to the game on the steps the simulated round
plays (a hand's score and the state it leaves, a discard, a blind's setup) is checked by the
parity tier where a change touches them (amended 2026-10-23, D17: before, it too rested on the
owner and the hands log). The quick pass takes 1-60 s a state
by the Ante (median 2.6 s; all 353 in 58 min), hence the sample (about 20 minutes each side;
the judge about 30 s a moved pick).

**Revisit if** most moved picks are ties flipping rather than changes, the oracle's verdicts
disagree with the owner's on cases the owner can explain, the sample misses regressions the
owner then finds in the states left out, or a stronger independent judge (a deeper search,
real outcomes) becomes cheap enough.

**Amended 2026-11-03: the history check where it can see the change, the checks once a review.**
The history check is skipped, the commit saying why, when the change can't reach Best play's quick
pass in a blind, which counts no shops (a change only to the shop ranking's options, the money's
walk or the long run's shops), unless the snapshot moves a Best play pick; whether it reaches is
told by what the quick pass runs, not by the file (9e53def touched only `advise.rs` and
`advise/value.rs` and moved 3 picks, 1 BETTER, through the blinds ahead Best play counts). The
snapshot isn't a stand-in where the change does reach it (2024566: no fixture's Best play moved,
the history 1 BETTER +5.5%). The checks run once before the first review, so the reviewer judges
their results; after a review's fixes only cargo test, clippy and the checks whose inputs the fixes
changed run again, and `cargo install` once at the end. The "same UTC day" for the two sides is
dropped: the sample leaves out today's saves, so a day between the sides can change only a few
states, and `history_judge` compares the states in both and lists the rest.
Why: the commits since 2026-10-09 (about 45 advice changes, the laptop's included). The history
check caught one bug no other check did (3965807, a pick 80% worse: the noise stage's equality on
rounds worth nothing), and once a known search miss (9648dde); none WORSE or LOST since 2026-10-14.
It moved no pick on 15 commits; on the changes confined to the shops it moved only ties and
unclear picks. What made cards long was rerunning the chain after each fix, a habit no rule asked
for. Revisit if a skipped change turns out to have moved a pick the owner then finds.

**Amended 2026-11-14: the history check on the states a change can reach.** It runs on the states
the change can reach, told by what its code does and widened to every kind of state the snapshot
moved, which runs first and on every fixture: `BAV_HOLDS` keeps the states holding a face-down
card, a boss, a big hand, or a joker or another game key, and `BAV_ONLY` takes several names
(`tests/common/filter.rs`, applied after the sample, so a filtered run's states and picks are a
part of the full run's). A kind is taken as the quick pass plays it, past the screen into the
round's later draws and the ante's blinds ahead (drawn cards turned face down are the four bosses
that do it, by their keys). The full sample runs when every state's analysis runs through the
change, and after three filtered checks in a row, with the last full one's commit as its before,
so a change that reached states its filter left out shows within four checks. The commit names
the check in a `History-check:` line, which `tools/status.py` reads.
Why: a change that could move only the states with a face-down card in hand, 4 of the check's
135 in a blind, replayed all 135 twice (about 40 minutes, 2026-11-03), and six face-down task
chats took 3.4-8.3 hours each, nearly all of it waiting on checks (D21's amendment). The risk is a
change that reaches states its filter left out: the snapshot in full shows a moved fixture of
another kind, the periodic full check what the fixtures don't hold. The cost is one full check in
four (15-30 minutes a side: the 134 states in a blind took 29 minutes on the main PC under load,
2026-11-14; and the judge on the moved picks of the changes since), against a filtered
check that takes what its states take. The snapshot in full stays (about 16 minutes a side on the
main PC, 2026-11-14), now the larger part of a filtered change's wait.
Revisit if a full check finds a WORSE pick outside the filters of the checks before it, or the
filtered checks keep needing most of the sample anyway.

Approved by the owner 2026-10-09; the limits amended with D17, approved 2026-10-23; the 2026-11-03
amendment approved 2026-11-03; the 2026-11-14 amendment approved 2026-11-11 (D21's amendment, "ok
with all 3").

## D13. The test suite runs in the test profile, with overflow checks (accepted)

**Decision.** Step 4's test run is `cargo test`, not `cargo test --release`. The test profile
(`Cargo.toml`: `opt-level = 3`) runs the same code as release (no `debug_assert` or
debug-only paths), but an integer overflow panics instead of wrapping. The slower checks that
compare outputs (the snapshot diff, the history check, the player benchmark,
`search_against_reference`) stay on `--release`. The release profile keeps wrapping: it is the
binary the live page runs, and a panic mid-run loses the analysis on screen.

**Why.** 446f0ea: the straight flush plan's room underflowed when a straight flush was held
whole. A debug build panicked (a reviewer reached it through `round_odds` → `decide`); every
required check ran in release, where it wrapped silently, so none could catch it, and the test
added for it passes in release with or without the fix. Under the test profile the whole lib
suite took about as long as in release (344 s against 315 s, the machine under load).

**Revisit if** the test profile gets much slower than release, or debug-only code paths
appear (then the two profiles no longer run the same program).

Approved by the owner 2026-10-14.

## D14. A fixture's expectation that waits on a known gap is kept as pending, and fails the test once it holds (accepted)

**What.** When a correct fix can't meet a replay fixture's expectation because of another
known gap, the fixture's `expect` is narrowed to what still holds and the rest moves to
`"pending_expect"`, with `"pending_gap": "gap N"`; the gap in `docs/design.md` names the
fixture, and the fixture's note says why. The replay test checks both: `expect` must hold, and
the test fails once all of `pending_expect` holds, so the fix that closes the gap is told to move
it back into `expect`. A `pending_expect` that isn't a non-empty object, or another key starting
with "pending", fails the test. This replaces the `pending/` folder (a fixture there wasn't run
at all; its one fixture, gap 25's, moved to the new form).

**Why.** The owner's fixtures are the evidence that a line is right; dropping an expectation to
make a change pass loses it silently, and keeping a failing fixture blocks every change behind
one gap. Raised 2026-10-14: finishing by the round's value (gap 18) moved
`head_keep_spades_and_seal` to a junk play because of another gap of the simulated player (gap
34: it never banks a free hand before the hand that wins); the owner agreed to ship with the
expectation narrowed "as long as the gap is clearly recorded so it can be reliably fixed later
and the fixture is handled properly now and during the fix".

**Revisit if** an expectation stays pending for long while its gap isn't being worked on (then
the gap's rank, not the fixture, is the problem), or pending fixtures pile up (then changes are
being shipped past the owner's evidence too easily).

Approved by the owner 2026-10-14: the fixture narrowed only with the gap clearly recorded, so
it can be handled later without confusion, now and during the fix; this mechanism is what that
requires (dropping or loosening the expectation, or parking the fixture where it isn't run, would
lose it).

## D15. The Hook's pick in an estimate: its average to choose by, its worst pair for "surely wins" (accepted)

Folded into `docs/design.md` gap 15 on 2026-10-17 (it records what a stage does, which needs no
entry under D16); kept so older commits and notes resolve.

## D16. What needs a decision: a change that makes a quoted sentence untrue (accepted, amended 2026-10-17, 2026-10-31)

**What.** An entry here only when a change makes a sentence untrue in `docs/design.md`'s four
stages, patterns or numbered rules, in the workflow in `CLAUDE.md`, or in a project-wide choice
(language, dependencies, scope); whoever proposes one quotes that sentence. A fix that carries
out a rule or extends how the code meets it (a new `Gain` field is rule 1's own path), or brings
the advice closer to the question every piece of advice answers, needs none, however large: it's
recorded in `docs/design.md` (its text, the register, the retire list, the known gaps, the
evidence there). "When the rules don't fit the fix" starts from that question. The reviewer's
check 8 flags a missing entry only with the quoted sentence and never offers "or record a
decision" as a fix. A change to a decision amends its entry in place, marked amended with the
date. One definition, in `CLAUDE.md` ("What needs a decision"); the reviewer's check 8 and this
file's header point to it. Amended 2026-10-31: a rule bent for now (until a queued fix) is named
in its gap, with no entry; the owner approves only scope, dependencies, what the advice values
and the workflow, and a technical rule changed for good is approved by the reviewer.

**Why.** First (2026-10-16): the bar was set in two places that disagreed: `CLAUDE.md` asked for
an entry when a fix can't be made within the design's rules, the reviewer whenever a change edits
`docs/design.md`, which nearly every fix does; and this file's header still spoke of phases,
retired in D6. Entries drifted from how the project works (D1-D14) toward single behaviours (D15,
an amendment to it, and a dig-plan change filed as a decision and folded back into gap 15).
Amended (2026-10-17): one definition wasn't enough, as it was a list of topics (the stages, the
rules, a `Gain` field) that nearly every fix touches, so a fix extending how the code meets a rule
read as a change to the rule. Twice in a day a fix was escalated: counting the blinds ahead in
Best play's round value was proposed as a new decision, though the founding question already
values a choice by its chance to get through times the long run, after the reviewer offered "or
record a decision" as an alternative. And the list contradicted rule 1, whose own path for a new
kind of gain (a `Gain` field) needed an entry. A quoted sentence is a test, not a topic.
Amended (2026-10-31): the owner was asked a third time to approve a technical rule they can't
judge (a temporary shortcut for a hand of only face-down cards, against rule 2's "never a cut
by the cards' order"), and said they didn't understand it; an approval that can't be judged adds
nothing, while the reviewers already check every change against the rules.

**Revisit if** a behaviour recorded only in `docs/design.md` gets reverted or re-argued because
no one could find why it was chosen, or a fix that broke a rule went ahead because no single
sentence was quoted.

Accepted by the owner 2026-10-16; amended with the owner's approval 2026-10-17 and 2026-10-31.

## D17. The game's own code as a test-only parity tier (accepted, amended 2026-10-23, 2026-11-02)

**What.** A test tier that runs the game's own Lua headless and checks the engine's steps against
it, lockstep, on states built from the owner's saves. A small LuaJIT program of our own
(`tools/parity/`: a stand-in for LÖVE, where any call returns a value that does nothing, plus a
job runner, about 220 lines in all, none of it the game's) loads the game's code at test time from
the owner's `Balatro.exe` as it is (read from the zip, nothing extracted or written), loads a saved
state through the game's own `Game:start_run`, takes one step through the game's own button
functions (`G.FUNCS.play_cards_from_highlighted`, `discard_cards_from_highlighted`,
`select_blind`, `cash_out`, `reroll_shop`, `use_card` on a booster), runs the game's event queue
until it rests and writes the game's own save of what the step left (`save_run`). A Rust test
(`crates/core/tests/parity.rs`, `--ignored`) pairs that save with the state before, as `handlog`
pairs the history's, and compares the engine's step with it: the score (`score_as_played`), the
state a hand leaves (`Board::after_hand`), a discard's (`Board::discard`) and a blind's setup
(`Board::at_blind_set`), every difference listed by cause. The harness rolls the game's own RNG
from the save, so a roll is the game's: the engine's every-roll-fails and every-roll-hits pair is
its range, as in the hands log. Two kinds of state: the history's own steps, where the game's next
save checks the harness, and generated ones from each saved state in a hand (the play the engine
scores most, a random play, a random discard), which reach plays the owner didn't make. It runs
where a change touches the engine or the steps the simulated round plays on (`engine/`, the
`Board` steps), before and after, as D12's slow checks run where they apply: every new difference
explained or fixed; and after a game update, which it follows by itself since it reads the install.

**Needs installed.** LuaJIT 2.0.5, the game's own (its `lua51.dll`, amended 2026-11-02: this
said 2.1), built from LuaJIT's git (`BAV_LUAJIT`); the system package's 2.1 runs too, with a
warning, since it orders a `pairs()` list of string keys anew at each launch and a pick from one
isn't the game's. Run as a process; the feasibility run used the copy in the
Lutris runtime already on this PC, through a 20-line C host, which isn't a setup to keep. Nothing
else but `unzip`, which the stand-in reads the game's files from the zip with, as CLAUDE.md reads
them (amended 2026-10-23, as built): the Rust test inflates the saves (`jkr`) and reads the
harness's as plain text. The owner's profile is read in place
from the save directory (unlocks shape the shop's pools), read only; the tier skips with a message
where the install, the profile or LuaJIT is missing (a cloud session, CI), as the history checks do.
Not taken: `mlua` with a vendored LuaJIT as a dev-dependency (in process, faster, but a C build and
a heavy dependency, CLAUDE.md asks first).

**How it keeps D2.** The game isn't an external engine but the ground truth D2 names, read in place
from the owner's install at test time as `extract-data` reads it; none of its code is copied,
extracted or committed (CLAUDE.md: never copy Lua into the repo: the repo's Lua is the stand-in and
the runner, new code). It is test-only: nothing the advice runs on calls it.

**Why.** The sentences this changes: D12's limits, "Shop choices, value across antes and the
engine's fidelity to the game still rest on the owner and the calibration log", and CLAUDE.md step
4's list of the checks a change runs (a new one where it applies). Measured 2026-10-23
(`docs/design.md`, the golden tests' paragraph): the harness reproduces the game's own next save on
2,970 of 2,984 of the history's steps (hands, round-winning hands, discards, blind setups,
cash-outs, shop rerolls, pack openings); of the other 14, 8 are the owner acting between two
saves, 3 the save itself (a rounded float, its timing) and 3 card ids (copies sharing one, a new
card's id from a counter the save doesn't keep); against it the engine scores 5,592 hands with no
mismatch (583 rolls in range), leaves the same state after 4,244 of 4,310 hands (the rest rolls in
range, a Glass card breaking, Crimson Heart) and all 2,308 discards, and sets 532 of 576 blinds the
same: the 44 are two engine gaps (the jokers' `setting_blind` context besides Burglar, and Juggle
Tag's `round_start_bonus`: queued) and bosses gap 15 plays as plain blinds. Each step costs 0.1-0.5
s with its load and save (0.6 s to start the game once); the 11,000 jobs of the measure took about
20 minutes on 4 cores at nice 19. The hands log checks only the hands the owner played, and only
their score; the parity tier checks every step the simulated round plays, on boards the owner
never played, with the game's own rolls.

**For step 8 (a simulated whole run).** With the owner's profile, the harness reproduces the
game's shop after a cash-out (134 of 139, the rest the owner acting first; 22 of 117 differed
without the profile), every reroll (150 of 150), every pack's contents (443 of 443), the next
blinds and their tags, from the RNG state a save holds. So a simulated whole run's shops, packs and
bosses can be checked lockstep against the game from any saved state, without the game running or
the PC idle, or the harness could itself drive whole runs headless for a run bench. Not tried yet:
buying, selling, using a consumable, skipping a blind (other button functions, to be driven the
same way); a state the game ends on a UI screen (the win screen) rests there; the time a step takes
in a run kept in memory (each job above loads a save and writes one).

**Revisit if** keeping the stand-in working takes more than a few lines a game update, the engine's
differences stay explained for long enough that the tier runs only after game updates, or an
engine that scores in game order with a usable license (D2) makes the game's own code unneeded.

Approved by the owner 2026-10-23, as proposed.

## D18. The private fixtures live in a private git repo every machine pushes to (accepted)

**What.** `tests/fixtures/private/` (still git-ignored here) is a clone of the private GitHub repo
`Timo425/balatro-advisor-fixtures`. Every machine pulls it before a task (`tools/pull-fixtures.sh`)
and commits and pushes a fixture it captures or changes, alongside the advisor commit it goes
with. Real runs stay out of this repo; they are committed to that private one. The main PC's
Proton Drive backup stops being how fixtures travel (CLAUDE.md, "Private fixtures").

**Why.** Fixtures reached other machines only through the main PC's one-way backup, and nothing
could go back, and nothing showed when that one path failed: `house_all_face_down_burglar`
(95a831e, from bot run BAVANG10) is named in gap 25 and its queue card, yet on 2026-10-31 neither
laptop nor the backup had it (the backup's newest fixture was from 10-29). It was only on the
main PC, with four fixtures edited there on 10-30: since the 10-30 reboot the backup's watcher
(a systemd path unit) had armed 31 s before the disk holding the repo mounted, so it watched the
empty mountpoint and never fired; nothing said so. Fixtures are small, edited text whose `expect` changes with commits: git
gives every machine a way to push, shows conflicts instead of a newer copy silently winning
(`rclone --update`), and its history ties a fixture's version to the code it matches.

**Revisit if** the fixtures stop being small text (then large append-only data, like the history,
needs another way), or real-run data must stay off GitHub even in a private repo.

Approved by the owner 2026-10-31 ("make a private git repo for fixtures").

## D19. Overnight bot runs are allowed (accepted)

**What.** CLAUDE.md's "Ask before a run that holds the game or the PC for hours, saying how long;
no overnight runs (suspending the PC at bedtime is fine)" loses "no overnight runs": a run that
holds a machine for hours, overnight included, is asked for like any other long run.

**Why.** The bot's series (`tools/botrun.py`) measure the advice in whole runs, minutes each, and
the main PC can't stay on at night, so the owner set up the Windows laptop to play them overnight.
A ban on the one time a machine is otherwise idle stopped those measurements without protecting
anything the ask before a long run doesn't already protect.

**Revisit if** an overnight run costs something the ask can't foresee (a machine the owner needs
in the morning, a run left holding the game).

Approved by the owner 2026-10-31 ("no overnight runs is a dumb rule get rid of it").

## D20. A card is tested by what it changes, at the level its question needs; bot runs only where earlier levels can't show the effect (accepted, amended 2026-11-10)

**What.** Three levels of test, each for its own question (CLAUDE.md, "Test a card by what it
changes"): snapshots before every commit (the advice: step 4's tests and history check; the bot:
`tools/botcheck.py` on the moments a bot run kept); one short live run for a change to the bot;
many runs only when the levels above can't show a card's effect, on the seeds where it shows,
sized to it. A card that wants runs names what it needs the bot to do; where the bot doesn't yet,
the bot's card goes first as an `After:` (written if missing). Which saved data serves what (the
owner's games, the bot's, fixtures) is in the same CLAUDE.md rule. The sentences this makes
untrue: CLAUDE.md's "Ask before a run that holds the game or the PC for hours, saying how long"
(now: over an hour, and why shorter won't do); docs/design.md's "Since 2026-10-30 paired whole
runs (…) judge changes whose effect shows antes later".

**Why.** The bot is a thin driver of the advice and a work in progress. One paired series (25
seeds before and after, 10-20 hours) judged the rent fix on 2026-10-30 and was then copied into
ten cards, bot changes among them: a card teaching the bot to buy vouchers asked for 14 hours to
see it do that. Meanwhile bot changes had no regression check at all. One different buy early
makes the rest of a run another game, so whole runs can't compare old and new decision by
decision; snapshots can, in minutes. A fixed "until the bot is mature" gate would be as arbitrary
as the 25 seeds: what a card needs from the bot is a fact per card, and making the bot do it is
queue work. The goal (docs/owner-view.md) is an advisor as good as the owner or better (said
since 2026-11-10 as beating Gold Stake, that being a goalpost: D21); hours of runs that tell
little take effort from the gaps.

**Revisit if** cards keep naming runs whose results don't move a decision, or the bot's snapshot
check misses what live runs then catch.

Approved by the owner 2026-11-02 ("bot running is proportional to the task and to the bot's
current state"; the plan with the three levels and per-card preconditions).

## D21. The advice's time is weighed against what a change moves; work is placed by what it's worth to the goal (accepted, amended 2026-11-11)

**What.** Four changes to the workflow, and the goal said as the owner means it.
- The goal (CLAUDE.md's first line, `docs/owner-view.md` 2026-11-10): advice that beats Gold
  Stake, first with the advisor at that level, later with the bot playing a run through on its
  own; then better and faster, learning from better players. "As good as the owner" is a goalpost
  on the way.
- Step 2 says what a fix can be worth at most before it's built (how often its situation comes
  up in the saved games it is about, how far the pick, the chance or a run's money can move
  there); a small ceiling on a gap the owner hasn't rated high is recorded and not built.
- Step 4 times old and new on the same saved states, one after the other (`tools/timing.py`, ten
  states and the builds alternating, each state's fastest of two runs). Two lines: a limit
  against the base, 10% on a group's total (one build's totals repeat within 0.4-2.3% on the
  set; a starting line), over which the commit says what the time buys; and a budget in seconds
  for each machine (`tools/timing-states.txt`), which a row of changes each inside the limit
  can't creep past and only the owner moves. Set 2026-11-10 to what the build of 2026-11-07
  takes on the main PC (30 s for the five hands, 120 s for the five shops): no slower than
  today (the code of 761226d, timed 2026-11-11: 27.5 and 117.7 s; the budget raised that day
  to 33 and 130 s, since 120 s left the shops 2% of room, about the totals' run-to-run spread). The reviewer's ninth check weighs cost against effect.
- A follow-up found while working a card is parked at the queue's end ("Found along the way"),
  not put on top, and moves up on the owner's word or as a prerequisite; what is left of a gap
  is one card there (the queue's "Placing a card").
- Reading: `docs/design.md`'s top and the gaps a card names, not the whole file.

The sentences this makes untrue: CLAUDE.md's "an advisor as good as the owner or better, and
then learning from better players" (as the whole goal); "Read it before changing what the advice
says" and "the whole `docs/design.md`" (now its top and the gaps named: the file is 374 kB, about
140,000 tokens, 69% of it the gaps' dated records); the queue header's "at the right place" (now
defined).

**Why.** Nothing limited how long the advice takes or said how big an effect must be to be worth
building, while every change to it was proven, reviewed and reviewed again: guarded against
half-done work only. Measured 2026-11-10 (the logs as they stood; the builds named below):
- In the owner's logged play (2026-10-09..22) a shop's analysis finished in 129 of 538 shop
  visits; every stop was a newer save, 4 to 5 s after the one before at the median, against a full
  pass of 20 s at the median: the shop advice mostly never reached the screen. A blind's quick pass
  finished 78% of the time. (Amended 2026-11-12: with a shop's first ranking shown first, a ranking was
  made before the next save on 39 of 70 sampled first shop screens on the owner's laptop (one with
  the finished top on 22), the whole pass on 24; at the main PC's speed about 34 to 37 (17 to 20);
  `docs/design.md` "Where things stand".)
- The build of 2026-10-22 (a46578b) against the one installed 2026-11-07 (98d7f26), same PC,
  idle, back to back, two runs each: five hands in a blind on the quick pass 22.0 → 29.2 s in
  all (+33%), five shops on the full pass 95 → 118 s (+24%); ten states, one PC.
- One of the owner's hands at The Mark with three of nine cards face down (7JR7ZTZ6 Ante 7,
  the quick pass): 23 → 257 s. The newer build no longer reads the face-down cards, the gain;
  the round is won 99.8% and 100%. 70 of the owner's 2,334 saved states in a blind hold a
  face-down card; gap 25 was named in 23 of the 42 commits that named a gap in the 14 days
  before, and held five of the queue's first eight cards.
- Each change that added to it said its own cost in its commit ("about twice as slow" 4428670,
  "20-28% slower" 98d7f26) and none had a line to hold it against.
The owner's rating "highest" for gap 32 had no card at all, and the economy cards the owner's
own words point at ("doesnt seem to have any strategy [...] building joker set and economy
properly") sat from the eleventh place down: follow-ups went on top of the queue because the
card that found them was there.

How long is worth it depends on what the time buys (the owner: "a balance thing, wouldnt want a
shop to take 20 mins when a 30 second shop can do most of the same kind of thing"), so the
budget is the owner's to move and starts at today's times, not at a number picked for them. A
per-state flag isn't used: one run each, 21 of 67 states came out slower after a change that
only cut work (gap 20's record). The bot needs the speed as much as the page: its runs are
91-97% analysis (7.3 of 7.8 minutes at the median over 190 logged runs, already at 100 rounds
a board against the page's 300), and the owner wants a win to take less than a day.

**Revisit if** the timing set stops looking like the states the owner plays (it is ten states
of October 2026, kept beside the history so its pruning doesn't take them), the limit keeps
being crossed by changes worth keeping, the ceiling in step 2 keeps cards from being built that
later turn out to decide runs, or parked follow-ups pile up unread.

Approved by the owner 2026-11-10 ("yes all five and also consider what i wrote now": the
goal, the balance of speed and strength, and the queue stacked on one task's sub-tasks).

**Amended 2026-11-11.** Three changes, the owner not wanting to order or judge cards.
- The queue's order is set by a direction check after every five finished cards (the queue's
  header), not on the owner's word card by card: where things stand on recent code, the cards
  above the parked ones put in order, the owner told in five lines. It writes no cards and puts
  none on top for being new. This replaces "moves up on the owner's word" above; the owner can
  still change the order by saying so.
- A card has a time box (CLAUDE.md, "A card has a time box"): a line at about four hours, the
  leftovers parked; at about eight the rest goes back as the card. Six task chats on the main
  PC (2026-11-04..07, all face-down work) took 3.4-8.3 hours each, nearly all of it waiting on
  the checks before and after.
- The checks may run on just the states a change can reach (done 2026-11-14: D12's amendment of
  that date).

Parking isn't free: a parked card done later is read into anew. It pays when the card is never
needed, which is what an edge case's leftovers should mostly be; a card's main effect isn't
parked by the time box.

Approved by the owner 2026-11-11 ("order without me is better yes, but just order and dont
randomly throw cards at the top. ok with all 3").
