# balatro-advisor: plan

A fast local tool that reads a vanilla Balatro save and answers three questions:

1. How much does each current joker contribute?
2. What is their best order?
3. Which shop or candidate joker would most improve the run?

It is a library first. The CLI and the MCP server are thin entry points over
the same API, so a separate agent repo can call it.

Background: [decisions.md](decisions.md) (language, external engines),
[formats.md](formats.md) (save/profile layout, verified vs not).

**Inputs from the owner's Obsidian notes** (`Arvutimängud/Balatro/overview.md`,
`learned.md`) that shape the design:

- *Calibration*: past advice said things like "easily" and "100%" and then
  missed. So every number carries its sample count and a ± interval, blind
  outcomes are given as probabilities, and the text output never uses
  certainty words unless P ≥ 95%.
- *The save is a checkpoint, not live state*: `analyze`/`watch` show the
  snapshot's age and what it cannot contain.
- *Verified mechanics* go into tests: Ancient Joker's ×1.5 per card multiplies
  everything to its left (so play order matters), and Amber Acorn shuffles
  what you see, but the save keeps the real order. For the gold tracker: every
  joker in the slots at a win gets the sticker, even if debuffed, and a Mr.
  Bones save on the ante-8 boss counts as a win.

---

## 1. Architecture

One Cargo workspace (see D1 for the toolchain question).

```
balatro-advisor/
├── crates/
│   ├── core/            library crate `balatro_advisor`, everything below
│   │   ├── jkr/         .jkr codec + Lua-table parser → LuaValue
│   │   ├── save/        LuaValue → typed RunState, Profile; path discovery; config
│   │   ├── model/       Card, Joker, HandLevels, Blind, Stake, Deck: pure data
│   │   ├── rules/       hand detection (with rule flags), blind modifiers
│   │   ├── engine/      the scoring pass, joker effect implementations
│   │   ├── sim/         Monte Carlo: sampling, best-play search, round sim
│   │   ├── advise/      contributions, ordering, shop ranking, roles, projections
│   │   └── gold/        profile → gold stake progress
│   ├── cli/             `balatro-advisor` binary: analyze|shop|gold|watch|bench|score
│   └── mcp/             `balatro-advisor-mcp` binary: MCP server over stdio
├── data/
│   ├── jokers.base.json    generated from the local game: key, name, rarity, cost, config, compat flags
│   └── jokers.toml         hand-written: role tags, effect impl id + params, notes
├── tests/golden/           in-game hands with the real score (see §7)
├── bench/history.jsonl     one line per bench run (commit sha, timings)
└── tools/extract_jokers.*  dev script: Balatro.exe → data/jokers.base.json
```

**Library API.** This is the stable surface, and the JSON types are these
structs serialised with a `schema_version`:

```rust
let state   = save::load(&SaveLocator::discover(profile)?)?;        // RunState
let opts    = SimOptions { samples: 1000, seed: 42, early_exit: true, .. };
let report  = advise::analyze(&state, &opts)?;       // contributions + best order
let shop    = advise::rank_shop(&state, &candidates, &opts)?;
let gold    = gold::progress(&profile::load(..)?, &JokerDb::bundled());
let scored  = engine::score_play(&state.board(), &play, ScoreMode::Trace); // one hand, step-by-step
```

`RunState` can also be built from JSON (`RunState::from_json`). The agent can
then ask "what if" questions about hypothetical boards without a save file.

---

## 2. State model (phase 2)

Parsed from `save.jkr` into typed structs. Anything we do not understand is
kept in an `unparsed` map, not dropped.

- **Jokers**, in slot order: key, edition (foil/holo/poly/negative), stickers
  (eternal/perishable with rounds left/rental), debuffed, sell value, and
  **internal counters** (`ability.mult`, `x_mult`, `chips`, `extra.*`). Also the
  per-round targets from `current_round` (Ancient suit, Idol card, Castle suit,
  Mail rank) and To Do List's hand.
- **Deck**: every card, with rank, suit, enhancement, edition, seal,
  `perma_bonus` and debuff, and which pile it is in (draw, hand, discard,
  played).
- **Hand levels**: level, chips, mult, and times played (overall and this
  round).
- **Economy**: money, interest rate and cap.
- **Run**: ante, round, blind choices and states, boss key, blind target
  (stake scaling × deck `ante_scaling` × blind multiplier), score so far,
  hands and discards left, hand size, joker and consumable slot limits.
- **Shop**: jokers with cost, packs, vouchers, reroll cost. Pending edition
  tags are applied the way the game will apply them right after the save
  (logic from `balatro_state.py`).
- **Other**: stake, deck (Back), vouchers, consumables, tags, skips.
- **Snapshot meta**: file age, screen, and "what this snapshot cannot show"
  (buys/sells/consumables since the last checkpoint).

Every field is verified against a fresh real save before it is relied on.
Each joker's counter field gets one line in `jokers.toml` (`state = "mult"`,
`"x_mult"`, `"extra.chips"`, …), not a special case in code.

---

## 3. Scoring engine (phase 3)

A **pure function** `(Board, Play, &mut Rng | ExpectedValue) → ScoreResult`
that follows `G.FUNCS.evaluate_play` in `functions/state_events.lua`. It is
verified line by line against that function, and the order ends up written in
`docs/scoring-order.md`. The outline, from reading it for this plan:

0. **Detect the hand** (`get_poker_hand_info`) with rule flags: Four Fingers
   (4-card flush/straight), Shortcut (straights with a gap of 1), Smeared Joker
   (red/black suits merge), Pareidolia (all cards are faces), and Wild and
   Stone cards. Output: the hand name, **all contained hands** (Jolly's
   "contains a Pair" etc.) and the scoring cards. **Splash**: every played card
   scores. **Stone** cards always score, and are inserted then **sorted by
   position** (the game sorts `scoring_hand` by screen x).
1. **Boss hand debuff** (`debuff_hand`: Psychic, Eye, Mouth, …) → score 0, but
   `debuffed_hand` joker triggers still run.
2. **`before` context**, jokers left to right: in-hand scaling (Green Joker,
   Ride the Bus, Spare Trousers, Runner, Square Joker…), which updates their
   counters *before* they score this hand. Card-changing effects (Midas Mask,
   Vampire) also fire here.
3. **Base**: hand level chips and mult (The Flint halves them).
4. **Per scoring card**, in position order. Repetitions = 1 + red seal + joker
   retriggers (Hanging Chad, Sock and Buskin, Seltzer, Dusk). Each trigger:
   card chips (rank + `perma_bonus`), enhancement (Bonus, Mult, Glass ×2,
   Lucky), card edition (foil +50, holo +10, poly ×1.5), then **on-scored
   jokers** left to right (Greedy family, Scholar, Photograph, Walkie
   Talkie, …). Debuffed cards score nothing and trigger nothing.
5. **Held in hand**, per held card: Steel ×1.5 and on-held jokers (Baron,
   Shoot the Moon, Raised Fist). Repetitions from red seal and Mime.
6. **Jokers left to right** (the loop also covers consumables, for
   Observatory): edition foil/holo → the joker's own effect → other jokers'
   `other_joker` effects on it (Baseball Card) → edition **polychrome ×1.5**.
   Blueprint/Brainstorm resolve to the copied joker's effect in this slot.
7. **Deck final step** (Plasma: balance chips and mult).
8. **Score** = `floor(chips × mult)`.

**Randomness** (Lucky, Misprint, Bloodstone, Space Joker, glass breaking) has
two modes: `Sample` draws from the run's RNG, and `Expected` uses exact
expectations where they are linear, for quick deterministic numbers. Oops! All
6s doubles numerators, as in the game.

**Trace mode** records every step (`source, chips_before/after,
mult_before/after, retrigger`). The golden-test harness uses it, and so does
`score --trace` for debugging a mismatch.

**Joker implementations are data-driven.** `jokers.toml` maps each joker to an
implementation id plus parameters. Many jokers share one implementation:

```toml
[j_jolly]      role = ["scoring"]  impl = "contains_hand_mult"  params = { hand = "Pair", mult = 8 }
[j_sly]        role = ["scoring"]  impl = "contains_hand_chips" params = { hand = "Pair", chips = 50 }
[j_duo]        role = ["scoring"]  impl = "contains_hand_xmult" params = { hand = "Pair", x = 2 }
[j_greedy_joker] role = ["scoring"] impl = "suit_scored_mult"  params = { suit = "Diamonds", mult = 3 }
[j_green_joker] role = ["scoring","scaling"] impl = "green_joker" state = "mult"
[j_mr_bones]   role = ["survival"] impl = "none"  note = "Prevents one death if ≥25% of blind is scored"
[j_golden]     role = ["economy"]  impl = "none"  econ = { per_round = 4 }
```

Numbers (`mult = 8`) default to `jokers.base.json`, which is generated from
the game, so a typo cannot drift from the real config. Only `config` is
trusted. The center's `effect` string is stale in places: The Duo says
`effect = "X1.5 Mult"`, but its config is `Xmult = 2`. **All 150 jokers get a
role row** (cheap, it is only data). A joker whose `impl` is missing is
reported as **`not modelled`** in every output, and the report says the
numbers exclude it. It is never silently ignored.

**First batch (~30 implementation ids, ~55 jokers).** Picked from the owner's
own `joker_usage` counts (top of the list: Abstract, Blue Joker, Swashbuckler,
Square, Supernova, Gros Michel, Green Joker, Ride the Bus, Card Sharp, Hanging
Chad, Raised Fist, Hologram, …), plus everything that changes hand detection:

| Group | Jokers |
| --- | --- |
| Flat / conditional | Joker, Jolly/Zany/Mad/Crazy/Droll, Sly/Wily/Clever/Devious/Crafty, Duo/Trio/Family/Order/Tribe, Half, Abstract, Banner, Mystic Summit, Misprint, Blue, Swashbuckler, Supernova, Gros Michel, Cavendish, Ice Cream, Stencil |
| Per scored card | Greedy/Lusty/Wrathful/Gluttonous, Scary Face, Smiley, Even Steven, Odd Todd, Scholar, Walkie Talkie, Photograph, Fibonacci, Arrowhead, Onyx Agate, Bloodstone, Ancient, The Idol, Triboulet |
| Held in hand | Raised Fist, Baron, Shoot the Moon |
| ×Mult from state | Card Sharp, Blackboard, Hologram, Constellation, Throwback, Glass Joker, Steel Joker, Driver's License |
| Scaling (counter from save) | Green, Ride the Bus, Spare Trousers, Square, Runner, Wee, Castle, Hiker (card perma chips) |
| Retrigger | Hanging Chad, Sock and Buskin, Mime, Dusk, Seltzer |
| Copy | Blueprint, Brainstorm |
| Rule changers | Four Fingers, Shortcut, Smeared, Splash, Pareidolia |

The owner confirms or edits this list in review.

**Boss blinds that change scoring** are modelled in phase 3 because they are
simple and they change the answer: suit/face debuffs (Club, Goad, Window,
Head, Plant), Pillar, Flint, Psychic, Eye, Mouth, Needle and Crimson Heart
(expected value over the disabled joker). Others are listed as not modelled.

---

## 4. Monte Carlo evaluation (phase 3–4)

**What one sample is.** Deal `hand_size` cards from the deck (without
replacement) and find the **best legal play**, meaning the play with the
highest score among all 1–5-card subsets. The card order within the play is
chosen too: see "play order" below. Two views, both reported:

- **Typical hand**: deal from the full deck. This is "how strong is this board
  on an average hand", which is the right lens for joker value.
- **This blind / next blind**: a whole-round simulation. The current blind
  starts from the real hand, draw pile, score so far, hands and discards left.
  The next blind starts from a fresh full deck. The discard policy is a simple
  heuristic: keep the cards of the best partial flush/straight/set, discard up
  to 5, redraw. It is labelled as a heuristic. Output: **P(beat blind)**, plus
  the expected best-hand score with p10/p50/p90.

**Common random numbers.** Every joker configuration being compared (the
baseline, each removal, each candidate, each ordering) is scored on the **same**
sampled hands (same seed). Differences between configurations then have much
lower variance than the raw scores, which makes contributions stable with far
fewer samples and makes early exit meaningful.

**Pruning and caching.**

- Hand detection depends only on the cards and 5 rule flags, not on joker
  values. It is computed once per (sample, rule-flag set) and reused across
  all configurations that share those flags. Only configurations that add or
  remove a rule changer re-detect.
- Best-play search with an upper bound: before running a full pass on a
  candidate play, compute an optimistic bound for its hand type. Skip it if
  the bound is below the best score found so far. Identical (hand type,
  scoring multiset, played count) plays are evaluated once.
- Ordering search (§5) reuses the **top-k plays per sample** found under the
  current order, so it never repeats the subset search.

**Early exit.** Samples run in batches of 64 (in parallel with `rayon`). After
each batch, stop when every pairwise ranking we report has separated at 95%
confidence (on the paired CRN differences), or when the relative standard
error drops below 1%. The default cap is 2000 samples, configurable. The
output always says how many samples were used and the ± on each number.

**Reproducibility.** Default seed 42, with `--seed` and `--samples`. The same
save, seed and version give byte-identical JSON.

**Play order.** Within a play, card order matters for per-card ×Mult (Glass,
polychrome cards, Photograph's "first face"). The search uses the rule "+Chips
and +Mult cards left, ×Mult cards right, Photograph's face first", which is a
verified heuristic, and brute-forces the order only when the scoring set is ≤4
cards and contains ×Mult effects.

---

## 5. Advice (phase 4)

**Contribution** = E[score with all jokers] − E[score without that joker],
under the current order (the joker is removed, not replaced, and the others
keep their relative order). For rule changers such as Four Fingers, that also
covers the hands that stop existing. Also reported: share of total, and a
note when contributions overlap (the removals do not add up to the total when
×Mult jokers multiply each other). Optional `--shapley`, exact over 2^n
subsets for n ≤ 6, gives a fairer split at about 64× the cost.

**Ordering.** Enumerate permutations over equivalence classes, not raw jokers:

- jokers with no score effect (economy/survival) → any slot, fixed out;
- pure +Chips jokers commute with everything in the joker phase, so they
  collapse into one class;
- pure ×Mult jokers commute with each other;
- position-sensitive jokers (Blueprint, Brainstorm, and any joker whose
  edition or effect mixes +Mult and ×Mult) are permuted fully.

Each remaining distinct order is evaluated on the CRN samples, and the best
order is reported with its gain over the current order (± CI). 6 jokers are at
most 720 orders, and the classes usually cut that to under 50. If the
difference is inside the noise, the answer is "order doesn't matter here".
Note: joker order also changes the per-card phase (on-scored jokers run left
to right for each card), and that is covered because each order gets a full
pass.

**Shop / candidates.**

- Candidates are the current shop jokers, plus any given with `--candidate
  j_baron[:polychrome]`, plus pack contents if the save shows an open Buffoon
  pack.
- Free slot (or a Negative candidate): gain = E[with candidate at its best
  position] − E[current].
- Full slots: evaluate the candidate **as a replacement for each non-eternal
  joker**, and report the best swap and the joker it replaces.
- Each row shows cost, affordability, the interest impact (money after the buy
  vs the $5 steps up to the cap), rental and perishable stickers, and
  **★ missing Gold** from the gold tracker.
- Non-scoring candidates (economy/survival/utility) get a **labelled
  heuristic note** instead of a fake score delta, e.g. "Economy (heuristic):
  +$4/round ≈ $X over the remaining N rounds".

**Roles and projections.**

- Every joker is tagged `scoring | scaling | economy | survival |
  utility/deck-fixing | rule-changer`, and a joker can have several tags.
- **Scaling projection (estimate)**: current counter + per-trigger growth ×
  expected triggers over the remaining rounds. Stated assumptions: hands per
  round = the typical number used so far, the best-hand mix from the Monte
  Carlo, and no further level-ups. Shown as "≈ +X mult by ante 8 (estimate)",
  and never mixed into the contribution number.

---

## 6. Interfaces

**CLI** (every command takes `--json`, `--profile N`, `--save-dir`, `--seed`,
`--samples`):

| Command | Output |
| --- | --- |
| `analyze` | per-joker contribution (± CI), role tags, not-modelled list, best order + gain, P(beat current/next blind), snapshot caveats |
| `shop` | candidates ranked by marginal gain (or best swap), cost/interest notes, ★ missing Gold |
| `gold` | Gold stake progress: have/missing by rarity, tally check against the profile |
| `watch` | re-runs `analyze` (or `shop` on the shop screen) whenever `save.jkr` changes; debounced (the game writes it several times in a row) |
| `bench` | fixed synthetic boards, timings per query type, appended to `bench/history.jsonl` |
| `score` | score one specified play, with `--trace`; the golden-test entry point |

**MCP server** (`balatro-advisor-mcp`, stdio). Tools map 1:1 to the library:
`get_run_state`, `analyze_jokers`, `rank_shop`, `gold_progress`, `score_hand`,
`evaluate_board` (takes a `RunState` JSON for what-ifs). The results are the
same JSON as the CLI.

**Note for the consuming agent.** `balatro-agent`'s rule is "Python never
decides; no ranking of siblings". This tool ranks by design. If
`balatro-agent`'s decision bench is the consumer, it will need an `--unranked`
mode that returns per-option absolute numbers in the game's fixed order. That
is cheap to add, and we will add it as soon as the consumer is confirmed.

---

## 7. Testing and correctness

- **Unit tests per joker**: a hand-built board and play, with the expected
  score derived by hand from the game source. Each test cites the Lua line it
  checks.
- **Hand detection tests**: every hand type × every rule flag combination
  (Four Fingers + Shortcut straights, Smeared + Wild flushes, Splash + Stone, …).
- **Golden tests** (`tests/golden/*.toml`): the owner supplies the board
  (jokers in order with editions/state, hand levels, played cards in order,
  held cards, relevant counters) and **the score the game showed**. The harness
  runs `score --trace` and, on a mismatch, prints the trace next to the
  expected value.
- **Semi-automatic golden capture (phase 4, if the owner wants it)**:
  `watch --record` keeps consecutive `save.jkr` snapshots in the gitignored
  `local/`. Between two "new hand" saves, Δ `GAME.chips` is the score of the
  hand just played, and the played cards are the ones that moved to discard.
  `golden extract` turns such pairs into candidate golden cases for the owner
  to confirm. Hands with random effects are flagged.
- **Parser tests** use synthetic `.jkr` files that we build ourselves. **No real
  save or profile file is ever committed** (`*.jkr` is gitignored, and private
  local fixtures go in `tests/fixtures/private/`, also gitignored).
- **Data test**: `jokers.base.json` matches the local `Balatro.exe` when one
  is present (skipped otherwise).

---

## 8. Performance

Targets on this machine (12 cores): **< 1 s** for a typical `analyze` + `shop`
(5 jokers, about 5 candidates) and **< 2 s** for full ordering of 6 jokers.

- `bench` exists from phase 3, the first commit that can score anything. The
  boards are fixed and synthetic (no real save needed) and the seeds are fixed.
  It records p50/p95 wall time per query type, the samples used and the
  commit sha.
- Budget per stage: parse < 10 ms, hand-detection cache build < 50 ms,
  scoring passes the rest.
- Levers, in order: CRN + early exit → bound pruning → detection cache →
  class-collapsed permutations → `rayon`. SIMD or bitboards only if the bench
  says so.

---

## 9. Phases (stop for review after each)

| # | Scope | Done when |
| --- | --- | --- |
| **1** | Research, formats, language, this plan | **← we are here.** Docs committed locally |
| 2 | Toolchain; `.jkr` parser; `RunState`/`Profile` models; path discovery + config; `gold` command (text + JSON) | Parser tests green; `gold` matches the game's tally on the real profile; one real `save.jkr` read field by field and every field in formats.md marked verified |
| 3 | Hand detection, scoring engine, first ~30 implementation ids, trace, unit tests, golden harness, `bench`, `score` | All joker tests green; owner's first golden hands match; bench under target |
| 4 | Monte Carlo, contributions, ordering, shop ranking, roles, projections; `analyze`/`shop`/`watch` | Numbers stable across seeds (±CI); timings under target on a real save |
| 5 | MCP server + stable JSON schema (`schema_version`), library docs | An MCP client can call every tool; schema snapshot tests |
| 6 | **Heuristic, labelled as such**: planets (re-simulate with the hand levelled), card-modifying tarots, vouchers (Observatory, etc.), economy advice (interest steps, reroll value) | Each output carries a `heuristic` label |

---

## 10. Open questions for the owner

1. **Toolchain** (D1): OK to install `rustup` user-locally (~1 GB, no sudo)?
   The alternative is staying on apt Rust 1.75 with a hand-written MCP layer.
2. **The two related repos**: the prompt had `[REPO_1]`/`[REPO_2]`
   placeholders. I assumed `balatro-agent` plus the vendoring pattern from
   `sts2-advisor-service`. Was something else meant?
3. **Consumer**: is the future agent `balatro-agent` (which forbids ranking), or
   a new repo? This decides whether `--unranked` is needed from day one.
4. **A live save**: phase 2 needs a real `save.jkr`. Start any run on profile 1
   and leave it in the shop or during a blind. The file is read in place and
   never copied into the repo.
5. **First joker batch**: keep the table in §3, or swap in jokers you are
   hunting for Gold? Your missing-Gold list includes Baron, Mime, DNA, Four
   Fingers, Splash, Shoot the Moon, The Family, The Order and Superposition,
   among 44.
