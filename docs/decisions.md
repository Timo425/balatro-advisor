# Decisions

Each entry: what was decided, why, and what would make us revisit it.
Status is `proposed` until the owner signs off at the end of a phase.

---

## D1. Implementation language: Rust (proposed)

**Decision.** Write the engine, CLI and MCP server in Rust as one Cargo
workspace. Python bindings (PyO3) are an optional thin crate added later only if
the agent repo needs in-process calls. The CLI's `--json` output and the MCP
server are language-neutral anyway.

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

**Revisit if** the agent repo needs in-process Python calls on a hot path. In
that case, add a `pyo3` crate over the same library and keep the engine as is.

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
  owner's local install into `data/jokers.base.json`, and we commit that
  generated file (values only, no code). A test re-checks it against the
  install when one is present.

### Related local repos

The prompt named two related repos, but the names came through as unfilled
placeholders (`[REPO_1]`, `[REPO_2]`). This is what I read:

- `balatro-agent`: `tools/balatro_state.py` has the Lua-table parser, blind
  targets per stake scaling, the pending-edition-tag logic, "what the save
  hides" notes and exact flush odds. `sidecar/*` has hand evaluation and
  text-parsed joker effects. We **copy and adapt** the parser idea, the
  blind-target tables and the blind-spot notes into Rust, with an attribution
  comment. `sidecar/scoring.py` is not reused: it estimates from effect text
  rather than modelling the game.
- `sts2-advisor-service`: its vendoring pattern (`VENDORED.md`, an override
  layer over untouched upstream code) is the template if we ever vendor
  anything.

Neither repo is modified or imported.

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

The spec is in English, a separate agent will read the docs and JSON, and
`balatro-agent` already set that convention for Balatro work. User-facing CLI
text is also English. Easy to flip if the owner prefers Estonian comments.
