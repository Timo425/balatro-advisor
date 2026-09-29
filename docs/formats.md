# Balatro file formats

What we know about the files, where each fact comes from, and whether it has
been checked against the owner's real files. Game version at time of writing:
**1.0.1o-FULL** (from `settings.jkr`).

## Locations

| Platform | Directory |
| --- | --- |
| Linux, Steam + Proton (verified) | `~/.steam/debian-installation/steamapps/compatdata/2379780/pfx/drive_c/users/steamuser/AppData/Roaming/Balatro` |
| Linux, other Steam roots | same suffix under `~/.steam/steam/`, `~/.local/share/Steam/`, Flatpak `~/.var/app/com.valvesoftware.Steam/.local/share/Steam/`, or any library listed in `steamapps/libraryfolders.vdf` |
| Windows | `%APPDATA%\Balatro` |
| macOS (untested) | `~/Library/Application Support/Balatro` |

`2379780` is Balatro's Steam app id. Lookup order: `--save-dir` flag, then the
`BALATRO_DIR` env var, then `save_dir` in `~/.config/balatro-advisor/config.toml`,
then auto-discovery in the order above.

```
Balatro/
├── settings.jkr         global settings; ["profile"] = currently selected profile (1–3)
├── 1/ 2/ 3/             one folder per profile
│   ├── profile.jkr      career stats, joker/deck/stake wins  ← gold tracker
│   ├── meta.jkr         unlocks/discoveries/alerts
│   └── save.jkr         the run in progress (absent when no run is active)
└── Mods/, *.run         mod leftovers / BalatroBot logs; ignored
```

Profile 1 is typically the main profile.
`balatro-agent` uses profiles 2 and 3 for bot benchmarks. The tool defaults to
the profile named in `settings.jkr`, and `--profile N` overrides it.

## Container: `.jkr`

Raw DEFLATE (no zlib header, `wbits = -15`). It inflates to UTF-8 Lua source of
the form `return { ... }`. The table syntax is only
`{["key"]=value,[1]=value,...}` with string or integer keys. Values are strings
(with `\"` escapes), numbers (including `inf`, `-inf`, `nan` and exponents),
`true`, `false` and nested tables. **Verified**: `profile.jkr`, `meta.jkr` and
`settings.jkr` from profiles 1 and 3 all decode with this grammar.

Lua integer keys matter: `wins = {[8] = 3}` is keyed by **stake number**, not by
list position. The parser keeps integer keys as integers.

## `profile.jkr`: gold stake tracker

Top-level keys (verified, profile 1): `career_stats`, `deck_stakes`,
`joker_usage`, `stake`, `voucher_usage`, `challenge_progress`, `MEMORY`,
`consumeable_usage`, `progress`, `deck_usage`, `hand_usage`, `high_scores`,
`name`, `challenges_unlocked`, `last_choices`.

```lua
joker_usage = {
  j_sly = { order = 11, count = 531,
            wins   = { [2]=2, [8]=1, [7]=1, ... },   -- stake → number of wins
            losses = { [7]=12, ... },
            wins_by_key = {}, losses_by_key = {} },
  ...
}
```

The game's rule (`functions/misc_functions.lua`):

- `set_joker_win()` adds 1 to `wins[G.GAME.stake]` for **every joker in the
  slots** when a run is won, debuffed or not.
- `get_joker_win_sticker(center)` returns the **maximum stake key** in `wins`,
  and the sticker is `G.sticker_map[max]`. Stakes 1–8 are White, Red, Green,
  Black, Blue, Purple, Orange and Gold.
- ⇒ **Gold sticker ⇔ `8 ∈ joker_usage[key].wins`**.
- A joker that has never been used has **no entry** in `joker_usage`, so the
  list of all 150 jokers must come from our data file, not from the profile.

**Verified against the real profile 1:** summing the max win stake over all
jokers gives **966**, the same as the game's own
`progress.joker_stickers.tally = 966` (`of = 1200` = 150 × 8). The tracker
recomputes this tally on every run and warns if it disagrees with the file, as
a format-drift canary. Current state: 106 jokers with Gold, 44 without.

## `save.jkr`: the run in progress

**Not yet verified in this repo.** There is no run in progress, so no
`save.jkr` exists right now. The layout below comes from `balatro-agent`'s
`tools/balatro_state.py`, which was checked against real saves on 2026-09-23
and 2026-09-25. Phase 2 re-verifies each field against a fresh save before we
rely on it.

| Path | Meaning |
| --- | --- |
| `STATE` | screen: 1 selecting hand, 5 shop, 7 blind select, 8 cash-out, 9/10/15/17/18 packs, … |
| `ACTION` | e.g. `{type="use_card", card=<sort_id>}` when saved while opening a pack |
| `GAME.dollars`, `interest_amount`, `interest_cap` | money and interest |
| `GAME.stake`, `GAME.win_ante`, `GAME.round`, `GAME.skips` | run meta |
| `GAME.round_resets.{ante, blind_choices, blind_states, blind_tags}` | ante and the three blinds |
| `GAME.current_round.{hands_left, discards_left, reroll_cost}` | round counters |
| `GAME.current_round.{ancient_card, idol_card, castle_card, mail_card}` | per-round joker targets |
| `GAME.hands[<name>] = {level, chips, mult, played, played_this_round, visible, order}` | hand levels |
| `GAME.used_vouchers`, `GAME.bosses_used`, `GAME.modifiers.scaling` | vouchers, bosses seen, blind scaling |
| `GAME.chips`, `BLIND.{name, chips}` | score so far vs target in the current blind |
| `BACK.{name, effect.config}` | deck (e.g. `ante_scaling`) |
| `cardAreas.{jokers, consumeables, hand, deck, discard, play, shop_jokers, shop_booster, shop_vouchers, pack_cards}` | `cards` (numeric keys = order) and `config.card_limit` |
| card: `base.{value, suit}`, `ability.{name, set, perma_bonus, …}`, `edition.type`, `seal`, `debuff`, `facing`, `cost`, `sell_cost`, `label` | playing cards and jokers |
| joker `ability.{mult, x_mult, chips, t_mult, t_chips, extra, eternal, perishable, perish_tally, rental}` | joker state (which field holds which counter varies per joker, see phase 2) |
| `tags` | held skip tags; edition tags (Foil/Holo/Poly/Negative) change a shop joker **after** the shop save |

**When the game writes it:** entering the shop, reroll, pack open (just before
the pack opens) and pack close, blind select, skip, boss reroll, each new hand,
cash-out. Buying, selling and using a consumable **do not** save. So the
shop screen in the save can be stale, and `watch` must say how old the
snapshot is and what it cannot show (this logic comes from `balatro_state.py`'s
`blind_spots`).
