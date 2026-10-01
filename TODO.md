# To do

Ideas and known gaps, roughly in order. Keep new strategy in its own place: a separate
check that reuses the engine, not another term in the options ranking.

- **Rush check** (after calibration shows the base holds up): "skip every small and big
  blind to Ante 8", with perishables that only age on played rounds (Throwback lasts
  exactly its rounds), Throwback grown by the skips, and the chance to beat each boss on
  the way. Separate output; doesn't feed the options ranking or By Ante 8.
- **Fix chance for the next boss**: "the shops before it (with your money on rerolls)
  find what gets you through: X%". A forecast, so a separate line under the boss, never
  part of the win chance. Reuses the shop simulation behind the money value.
- **Value over the antes in between**: By Ante 8 looks at the end state (plus the next
  ante's boss in the ranking); a sooner gain (Constellation +×0.1 from a planet now, Glass
  before it breaks) counts only through that. Weigh options over each remaining ante, and
  treat "money buys it later" as less certain than a card on offer now.
- **Golden cases**: record a few real hands (`score --golden`, then `golden set`).
- **Calibration**: let the log collect a few runs; check predicted vs actual win rates.
- Rentals held "until the danger passes, then sold", not held all run.
- Spectral cards aren't valued.
- The round policy doesn't plan for The Eye / The Mouth; Hook, Serpent and face-down
  bosses aren't simulated.
- Long-run interest isn't in the money value (dropping under the interest line).
- Director's Cut: the boss rerolls in later antes (Ante 8's included) aren't valued.
- Split `advise.rs` (options, By Ante 8, outlook, tarots) once it settles.
