#!/usr/bin/env python3
"""Plays Balatro runs through BalatroBot with the advisor making every choice, and logs
how far each run got. For strategy experiments on Profile 3 (on Linux with Steam, start the game
with balatro-agent's tools/bot.sh first; on Windows botrun starts the installed game itself on
Profile 3 with %APPDATA%\\Balatro\\BotMods, Lovely's version.dll beside it, and closes it and
gives back the profile when done; on Linux without Steam it does the same natively, headless, with
LÖVE's official build, Lovely's liblovely.so and the mods in ~/.local/share/balatro-advisor/native/,
BALATRO_EXE and LÖVE's own save folder: `NATIVE`). Runs go to ~/.local/share/balatro-advisor/bot/ on the main PC; another
machine sets BAV_BOT_DIR to its own bot-<machine>/, a send-only Syncthing folder to the main PC
(its data folder's bot/ is the main PC's, copied in, and would overwrite its runs.jsonl).

    tools/botrun.py --runs 5 --stake WHITE
    tools/botrun.py --seeds BAV00001,BAV00002 --stake GOLD   # paired: the same seeds before and after

The advisor's simulations are seeded (`analyze --seed`, 42), so the same code on the same game
seed plays the same run. `--advisor-seed 43` is a placebo: a change that does nothing but flips
the picks noise decides, so its paired difference is the noise a real change is judged against.

Strategy "advisor": play the advisor's best play; in the shop take the advisor's top
option it can act on while it's worth it (clear of keeping the money by more than the
ranking's noise: the advisor's `beats_keep` on each option), else leave. A joker
bought into full slots sells the one the advisor's option names by place and key
(`sell_index`, `sell_key`).

A run measures the advisor only where the bot follows it. Read runs.jsonl's `not_followed`
(each time the advisor's top choice was one the bot can't take) before reading a run as a
verdict on the advice. As of 2026-10-24 the bot:
- follows: plays and discards (Best play's quick pass, 100 simulations where the live page
  runs 300); joker buys, with the sale the advisor names; planets; rerolls, as many a shop as
  the advisor ranks above leaving (since 2026-11-01; before, at most 2 a shop and only while $5
  stayed after it, and at most 8 actions a shop);
  Buffoon and Celestial packs and their joker and planet picks, with the sale the advisor
  names (made in the open pack, as the game allows; since 2026-10-31: before, every pick needing
  a sale failed and the pack was skipped uncounted, the series up to then included); the
  jokers' order after each shop (the advisor's `order`, one for the board: no order is advised
  per hand, docs/design.md gap 37); since 2026-10-31 (3127f3e), skip-or-play at each Small and
  Big blind (`blind_select`: one more full analysis a blind, ~8 s at 100 simulations; "close"
  plays; the run logs every verdict with both sides' values and its `skips`). Before, it played
  every blind unasked. Since 2026-11-02: vouchers the advisor values (not one whose note says
  "not valued"); shop tarots and spectrals that go on no card you pick (`TARGET_FREE`, from the
  game's data) and that the advisor values, bought and used at once, or, those that act on the
  cards in hand (`NEEDS_HAND`: Familiar, Grim, Incantation, Immolate, Sigil), at the next blind's
  start, as their value counted them; a planet or such a tarot you hold, used when its
  "(you have it)" option is worth it (Blue Seals' planets, which it had held unused, and The
  High Priestess's); a
  planet Best play says to use first; a Standard pack's playing-card pick (found by its label:
  the advisor offers no Standard pack in the shop and doesn't value the Standard Tag, so no run
  has opened one yet). Before, every voucher, tarot and spectral was left and every "use X first"
  ignored uncounted.
- doesn't: tarots and spectrals that go on cards ("needs targets": their cards aren't in the
  output, gap 31), the ones the advisor doesn't value ("not valued": Ectoplasm, Ouija), one that
  makes a joker with your slots full ("needs a free joker slot"), The Emperor ("makes tarots that
  need targets", `MAKES_TAROTS`), Arcana and Spectral packs,
  "use X first" for a tarot ("blind: use X first (needs targets)"), a skip whose tag is valued as a tarot or spectral pick (Charm, Ethereal) or not valued at all (Foil,
  Double, Juggle on a Small blind…), and a blind whose analysis failed: it plays. Skip-or-play
  doesn't read the tags held (gap 21). Vouchers move the money a run holds: compare runs on
  either side of 2026-11-02 knowing it.
  Counted in `not_followed`, as is a pack pick it couldn't make ("pack
  pick needing a sale", "pack pick refused", "pack pick not in the pack": the bot then takes the
  best pick it can above skipping; "pack pick refused after its sale": it skips the pack). Since 2026-10-25 it also sells a joker on its own when the
  advisor's "sell X" option clears keeping the money (`worth`).
- needs: the bot profile (3) with every joker unlocked, as the owner's (the advisor's shop pool
  assumes every joker unlocked, gap 38); Steamodded's guard against saving in an open pack
  lifted in the bot's own copy (BotMods/Steamodded/lovely/fixes.toml), or the advisor never
  sees a pack's cards; BalatroBot's `sell` allowed in an open pack in the bot's copy
  (BotMods/Balatrobot/src/lua/endpoints/sell.lua, `requires_state` and its `valid_state` check
  given SMODS_BOOSTER_OPENED; patched 2026-10-31, the original kept as sell.lua.bak), or every
  pick needing a sale is "pack pick needing a sale"; the same `sell` ending once the sold card is
  gone and its money paid, without its check that the row holds one joker fewer (patched
  2026-11-03): selling a charged Invisible Joker copies another joker into the row (card.lua
  `Card:calculate_joker`, `selling_self`), and the call never answered; and its `pack` letting a Negative joker into
  full slots as the game does (pack.lua, pack.lua.bak; button_callbacks.lua `can_select_card`)
  and ending a pick or a skip back on the blind select, where a pack a skip tag opened closes
  (button_callbacks.lua `end_consumeable`, `PACK_INTERRUPT`; patched 2026-10-31), or its call
  waits for the shop until the RPC times out; and its `save` writing once no blocking event is
  queued (at most 5 s, answered `settled`), as the live mod's file waits (save.lua, save.lua.bak; game.lua `Game:update_shop`;
  patched 2026-10-31), or a shop entered with a Rare Tag is saved before the tag makes its
  joker free (tag.lua `Tag:yep`) and the advisor reads the joker at full price; botrun also
  waits on it at each blind select (`settled`, since 2026-11-03), where a skip tag's next pack
  may still be on its way.
- the game's restart between runs (Linux, through Steam) waits for Steam to be logged on, and,
  when Steam holds the launch on a prompt (a failed Cloud sync, as after a suspend while it was
  still offline, 2026-11-02; a conflict; a shader update), says which and waits up to an hour,
  launching again every 2 minutes so a cancelled dialog works as well as an answered one
  (`restart_game`, `steam_held`).
- the bot's game, in its BalatroBot copy (BotMods/Balatrobot/src/lua/settings.lua, the original
  kept as settings.lua.orig; 2026-11-01/02): starts minimized but still drawn, so it can be
  restored to watch (`BALATROBOT_MINIMIZED=0` to show it); and reads the mouse as off-screen
  (`BALATROBOT_MOUSE=1` to use it), as a pointer resting where the deck is drawn opens the deck
  preview and the game then makes no Play/Discard buttons (game.lua `Game:update_selecting_hand`):
  every play and discard failed (BAVANG25, three times at its first discard, 2026-11-02).
- the bot's own save folder (since 2026-11-02; on Windows botrun makes it the first time,
  `own_save_folder`): with the bot's Profile 3 copied to BalatroBot/3
  beside the game's Balatro folder (its profile.jkr and meta.jkr, and a settings.jkr on profile
  3), BotMods/bav-bench switches the game there (conf.lua's identity), so its saves stay off Steam
  Cloud and never touch the owner's settings.jkr or profiles; botrun and bot.sh then leave the
  owner's files alone. Without the copy everything works as before. bav-bench also keeps the
  game's window out of the way at boot (unless `BALATROBOT_MINIMIZED=0` or headless): LÖVE's
  first window opens at 1280x720 (conf.lua; desktop-sized otherwise) and is minimized at the first
  line of main.lua, and the start-up `apply_window_changes(true)` doesn't call `updateMode`, which
  opened a second, monitor-sized window in front (2560x1440, full screen, measured 2026-11-02);
  now one window, visible ~80 ms. On Linux Lovely's console is off (`--disable-console`).
Since 2026-10-29 every state the advisor analysed in a shop or an open pack (since 2026-10-31 a
blind select too), and every state an
analysis failed on (`error`), is kept (`bot/shops/<seed>/a<ante>-r<round>-<n>-<screen>.jkr`;
`analyze --file` replays it; since 2026-11-02 with what BalatroBot showed the bot and the advice
beside it, for tools/botcheck.py's old-against-new check of the bot's own choices) and the run's log is whole: each shop analysis logs the money, the jokers' stickers and every option ranked.
Every run before 2026-10-24 (none has a `commit`) is marked `void` in runs.jsonl: every pack
skipped, the jokers never arranged, "won" read from a flag the game also sets on a loss, and
before 2026-10-23 35 jokers locked and an early advisor. Don't read them as evidence.

The game is restarted before a run once it holds more than a fresh one (`--restart-game`, Linux
with Steam, Windows and natively: it grows as it plays, a run from a fresh game ending at 0.6-3.1 GB, measured 2026-10-30); elsewhere a series stops between runs
past `--max-game-mb` and prints the seeds to resume with. The runs are seeded, so a restart
between them changes nothing. Natively (`NATIVE`) that needs the Windows game's own LuaJIT, 2.0.5,
in native/love/lib (the vault note says how it's built): LÖVE's own (2.1, Nov 2023) seeds its
string hashing anew each launch, so a list the game builds with `pairs()` over string keys comes
out in another order, and so does a pick from it with the seed's own random number: the hands an
Orbital Tag levels (UI_definitions.lua, `orbital_choices`) and To Do List's hand (card.lua);
BAVANG01 gave three different `orbital_choices` in three launches. With 2.0.5 (2026-11-02) all 25
BAVANG seeds start as on the main PC (`orbital_choices`, rolled at every blind, the same on the
23 with a reference), and BAVANG07 and BAVANG13 (each offered To Do List, its hand the same: the
end-of-round re-pick is the same pick, card.lua) played every logged step and kept state as the main PC's
runs of the same advisor; only the suits' colour (a display setting) and the passing `STOP_USE`
flag differ. So a native series refuses any other LuaJIT (`check_native_luajit`).

Restarting (`--restart-below`, off by default: the bot plays every run out, a baseline; every run
logs `ante2_reach` either way). The threshold 0.30, a heuristic, was judged 2026-10-28 at 1786431
on the owner's 51 saved runs (Anaglyph Deck, Gold Stake, played with older advisors): of the 44
that reached the Ante 2 blind select, the 7 below 0.30 (6 played out) won none and none got past
Ante 5; the 37 at or above got to Ante 5.4 on average with 6 wins, and one won from 0.36, so a
higher line restarts winners. A small sample. Above it reach barely orders how far a run gets.
At Ante 1 the reach can't tell starts apart (no scoring joker yet), so the rule waits for Ante 2.
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
import zlib

API = "http://127.0.0.1:12346"
VERBOSE = False
ANALYSIS_S = 0.0  # the advisor's share of a run's time
NOT_FOLLOWED = []  # the advisor's top choices the bot couldn't take, this run
FOR_BLIND = []  # consumables bought in a shop to use at the next blind's start (`NEEDS_HAND`), this run
HOME = pathlib.Path.home()
WINDOWS = sys.platform == "win32"
# Linux without Steam (the WSL laptop; vault Arvutimängud/Balatro/advisor-second-machine.md): LÖVE's
# official 11.5 build runs BALATRO_EXE natively with Lovely's liblovely.so preloaded, both in
# NATIVE_DIR. Not Ubuntu's `love`: it doesn't export PhysFS, which Steamodded's nativefs calls.
NATIVE_DIR = HOME / ".local/share/balatro-advisor/native"
NATIVE = (sys.platform.startswith("linux") and not shutil.which("steam")
          and (NATIVE_DIR / "love/bin/love").exists())
# BAV_BOT_DIR: a machine other than the main PC writes to its own bot-<machine>/ (the docstring)
OUT = pathlib.Path(os.environ.get("BAV_BOT_DIR") or HOME / ".local/share/balatro-advisor/bot")
STATE = OUT / "state.jkr"
KEEP_DIR = None  # this run's kept states (bot/shops/<seed>/): every shop and pack the advisor analysed
# The path the game saves to: on Linux with Steam it runs under Proton, where Z: is the Linux root
WIN_STATE = str(STATE) if WINDOWS or NATIVE else "Z:" + str(STATE)


class RpcError(Exception):
    pass


_id = 0


def rpc(method, **params):
    global _id
    _id += 1
    body = json.dumps({"jsonrpc": "2.0", "method": method, "params": params, "id": _id}).encode()
    req = urllib.request.Request(API, body, {"Content-Type": "application/json"})
    # A call BalatroBot never answers (its endpoint waits for a state that doesn't come) is an
    # RpcError: an action's (a sale, a buy, a play) is logged by `one_run` and the run goes on,
    # where before the exception stopped the series. A game that stops answering altogether
    # still stops it (`one_run`'s `menu` and its loop's `gamestate` aren't caught). Its
    # waiting event stays queued in the game and, should it pass later, answers whatever call is
    # open then (server.lua `send_response` takes the current request's id).
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            data = json.load(r)
    except TimeoutError:
        raise RpcError(f"{method}: no answer in 120 s") from None
    except urllib.error.URLError as e:
        if not isinstance(e.reason, TimeoutError):
            raise
        raise RpcError(f"{method}: no answer in 120 s") from None
    if "error" in data:
        raise RpcError(f"{method}: {data['error'].get('message', data['error'])}")
    return data.get("result")


ADVISOR_SEED = 42


# Steamodded opens every pack in one state of its own (SMODS_BOOSTER_OPENED = 999, its
# lovely/booster.toml); the vanilla game, which the advisor reads, has one per pack type
# (globals.lua G.STATES), keyed here by the set of the cards in the pack.
PACK_STATES = {"Tarot": 9, "Planet": 10, "Spectral": 15, "Joker": 18}  # else Standard: 17


def vanilla_state(path):
    """Rewrites a save the bot's modded game wrote in an open pack to the vanilla state of that
    pack, so the advisor sees the pack's picks. Returns the state written, or None."""
    raw = zlib.decompress(path.read_bytes(), -15).decode()
    if '["STATE"]=999' not in raw:
        return None
    i = raw.find('["pack_cards"]')
    m = re.search(r'\["set"\]="(\w+)"', raw[i:]) if i >= 0 else None
    if not m:
        return None
    state = PACK_STATES.get(m.group(1), 17)
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    path.write_bytes(c.compress(raw.replace('["STATE"]=999', f'["STATE"]={state}').encode()) + c.flush())
    return state


def keep_state(screen):
    """Copies the state about to be analysed to this run's folder (`a<ante>-r<round>-<n>-<screen>.jkr`),
    so a run's shops can be replayed with `analyze --file` afterwards: state.jkr is overwritten.
    Beside it, what BalatroBot showed the bot then (`.gamestate.json`); `analyze` adds the
    advisor's answer (`.advice.json`). With both, tools/botcheck.py re-runs the bot's own choices
    on the moment, old code against new, without the game. Returns the kept file's path."""
    if KEEP_DIR is None:
        return None
    gs = rpc("gamestate")
    n = sum(1 for _ in KEEP_DIR.glob("*.jkr"))
    path = KEEP_DIR / f"a{gs.get('ante_num')}-r{gs.get('round_num')}-{n:03d}-{screen}.jkr"
    path.write_bytes(STATE.read_bytes())
    path.with_suffix(".gamestate.json").write_text(json.dumps(gs))
    return path


def analyze(sims, quick=False, keep=None):
    global ANALYSIS_S
    saved = rpc("save", path=WIN_STATE)
    if "settled" not in (saved or {}):
        sys.exit("BalatroBot's save.lua isn't the patched copy (the docstring's \"needs\")")
    if not saved["settled"]:
        print("  (saved with events still queued after 5 s)")
    v = vanilla_state(STATE)
    kept = keep_state(keep) if keep else None
    if VERBOSE and v:
        print(f"  (pack state {v})")
    t = time.monotonic()
    out = subprocess.run(
        ["balatro-advisor", "analyze", "--file", str(STATE), "--sims", str(sims), "--seed", str(ADVISOR_SEED)] + (["--quick"] if quick else []),
        capture_output=True, text=True, timeout=300,
    )
    ANALYSIS_S += time.monotonic() - t
    if out.returncode != 0:
        # (the state it failed on, for a fixture: kept whatever the screen)
        keep_state("error")
        raise RuntimeError(out.stderr.strip()[-300:])
    if kept:
        kept.with_suffix(".advice.json").write_text(out.stdout)
    return json.loads(out.stdout)


# Tarots and spectrals that go on no card you pick: the game's data gives them no
# `max_highlighted` (data/game.json, as BalatroBot's `use` reads it), less Aura, which needs one
# card in hand all the same (card.lua `Card:can_use_consumeable`). Of those, the ones that act on
# the cards in hand can only be used in a blind (the same function, its SELECTING_HAND branch);
# the ones that make a joker need a free joker slot.
_GAME = json.loads((pathlib.Path(__file__).resolve().parent.parent / "data/game.json").read_text())
TARGET_FREE = {c["key"] for c in (_GAME["centers"] if isinstance(_GAME["centers"], list) else _GAME["centers"].values())
               if c.get("set") in ("Tarot", "Spectral") and "max_highlighted" not in (c.get("config") or {})} - {"c_aura"}
NEEDS_HAND = {"c_familiar", "c_grim", "c_incantation", "c_immolate", "c_sigil", "c_ouija"}
MAKES_JOKER = {"c_judgement", "c_wraith", "c_soul"}
# The Emperor's value counts the 2 random tarots it makes as used, and most go on cards: the bot
# would hold them unused, its consumable slots full, and buy no planet after
MAKES_TAROTS = {"c_emperor"}


def held(o):
    return o["label"].endswith("(you have it)")


def why_not_tarot(o, gs):
    """Why the bot doesn't take this tarot or spectral option (bought and used, or used from
    your consumables), as `not_followed` words it; None when it can. The advisor's own words
    decide "not valued" (its note: Ectoplasm, Ouija, ...)."""
    key = o.get("key")
    if key not in TARGET_FREE:
        return "needs targets"
    if key in MAKES_TAROTS:
        return "makes tarots that need targets"
    if "not valued" in o["note"] or "not modelled" in o["note"]:
        return "not valued"
    if key in MAKES_JOKER and len(cards(gs, "jokers")) >= (gs.get("jokers") or {}).get("limit", 5):
        return "needs a free joker slot"
    if held(o) and key in NEEDS_HAND:
        return "used in a blind"
    return None


def cards(gs, area):
    return (gs.get(area) or {}).get("cards") or []


def index_of_key(gs, area, key):
    for i, c in enumerate(cards(gs, area)):
        if c.get("key") == key:
            return i
    return None


def sell_joker(gs, o):
    """Sells the joker the advisor's option says to sell for it: by its place in your joker row
    (`sell_index`, the save's order, which is the game's), checked against its key (`sell_key`),
    so two copies or a label the game words differently can't sell the wrong one."""
    js = cards(gs, "jokers")
    i, key = o.get("sell_index"), o.get("sell_key")
    if i is None or i >= len(js) or js[i].get("key") != key:
        return False
    rpc("sell", joker=i)
    return True


def use_held(gs, key=None, label=None):
    """Uses the consumable you hold with this key (or label), the rightmost such: True when used."""
    cs = cards(gs, "consumables")
    i = next((i for i in reversed(range(len(cs))) if cs[i].get("key") == key or (label and cs[i].get("label") == label)), None)
    if i is None:
        return False
    rpc("use", consumable=i)
    return True


def play_hand(sims, log):
    gs = rpc("gamestate")
    # a spectral bought in the shop to use on a hand (`NEEDS_HAND`): at the blind's start, as its
    # value in the shop counted it (the round after it, its deck changed)
    while FOR_BLIND:
        key = FOR_BLIND.pop()
        try:
            if use_held(gs, key):
                log.append(f"use {key} at the blind's start")
                gs = rpc("gamestate")
        except RpcError as e:
            log.append(f"use {key} failed: {e}")
    a = analyze(sims, quick=True)
    bp = a.get("best_play")
    gs = rpc("gamestate")
    hand = cards(gs, "hand")
    if not bp or not bp.get("indices"):
        idx = list(range(min(5, len(hand))))
        rpc("play", cards=idx)
        return
    idx = [i for i in bp["indices"] if i < len(hand)]
    blind = next((b for b in (gs.get("blinds") or {}).values() if isinstance(b, dict) and b.get("status") == "CURRENT"), {})
    if VERBOSE:
        print(f"  ante {gs['ante_num']} {blind.get('name', '?')}: {gs['round'].get('chips')}/{blind.get('score', '?')} · "
              f"hands {gs['round']['hands_left']} discards {gs['round']['discards_left']} · {bp['action']} {' '.join(bp['cards'])} "
              f"({bp.get('hand', '')} {bp.get('score', 0):.0f}, win {bp.get('p_win')})")
    if bp["action"] not in ("play", "discard"):
        NOT_FOLLOWED.append(f"blind: {bp['action']} {' '.join(bp.get('cards') or [])}")
    # "use X first": a planet changes no card in hand, so the move's places stand after it; a
    # tarot Best play names goes on cards (its targets aren't in the output: gap 31)
    u = bp.get("use_first")
    if u:
        c = next((c for c in cards(gs, "consumables") if c.get("label") == u), None)
        if c is not None and c.get("set") == "PLANET":
            try:
                use_held(gs, label=u)
                log.append(f"use {u} first")
            except RpcError as e:
                log.append(f"use {u} first failed: {e}")
        else:
            NOT_FOLLOWED.append(f"blind: use {u} first ({'needs targets' if c is not None else 'not held'})")
    if bp["action"] == "discard" and gs["round"]["discards_left"] > 0:
        rpc("discard", cards=idx)
    else:
        rpc("play", cards=idx)


def worth(o, a):
    """The advisor ranks every option against keeping the money ("next round" / "skip the
    pack"). Worth taking: ranked above that and clear of it by more than the ranking's noise
    (the advisor's `beats_keep` on each option; from an older build, which has none, each
    option's `rank` above keeping the money's by more than `tie` of the top's, the advisor's
    `shop_tie`). When
    the round is in trouble the ranking goes by the score reached (`trouble`), and ranked above
    keeping the money is enough.

    Neither is a rule of the bot's own: `beats_keep` is the advisor's own test (advise.rs
    `clearly_better`: the option against keeping the money on the simulated rounds and shops
    both rest on, the register's "Shop ranking" entry in docs/design.md), and in trouble the
    advisor says its ties don't apply (advise.rs `ShopTie`), so its ranking is followed as it
    stands. What the bot
    decides by hand is only which side of a tie to fall on: keeping the money. Both belong to
    the advisor's ranking already; nothing here to move there."""
    options, noise = a.get("options", []), a.get("shop_tie")
    keep = next((i for i, x in enumerate(options) if x["kind"] in ("leave", "skip")), len(options))
    if options.index(o) >= keep:
        return False
    k = options[keep] if keep < len(options) else None
    if k is None or noise is None or noise.get("trouble") or o.get("rank") is None or k.get("rank") is None:
        return True
    # the advisor's own finding, where it gives one (`beats_keep`: above keeping the money by
    # more than its simulated rounds' noise and its margin for what no round shows, advise.rs
    # `clearly_better`)
    if o.get("beats_keep") is not None:
        return bool(o["beats_keep"])
    top = max(x.get("rank") or 0.0 for x in options)
    return o["rank"] - k["rank"] > noise["tie"] * top


# A safety stop for the shop loop, far above any real shop: the bot acts in a shop as long as the
# advisor ranks an action above leaving (`worth`), each action followed by a new analysis
SHOP_GUARD = 50


def shop(sims, log):
    failed = set()  # options the game refused this shop: not tried again
    shop_start = len(NOT_FOLLOWED)
    for n in range(SHOP_GUARD + 1):
        if n == SHOP_GUARD:
            # never in a real shop: rerolls cost more each time and money runs out first
            log.append(f"error in SHOP: {SHOP_GUARD} actions in one shop, left (loop guard)")
            break
        a = analyze(sims, keep="shop")
        gs = rpc("gamestate")
        money = gs["money"]
        log_shop(a, gs, log)
        rnd = a["rounds"][a["options_round"]] if a.get("rounds") else {}
        now, reach = rnd.get("p_win", 0), rnd.get("reach", 0)
        free = len(cards(gs, "jokers")) < (gs.get("jokers") or {}).get("limit", 5)
        acted, took = False, None
        top = next((o for o in a.get("options", []) if not o.get("unaffordable") and not o["label"].startswith("pick ") and worth(o, a)), None)
        if VERBOSE:
            print(f"  shop ${money} · now {now:.2f} reach {reach:.2f} free slot {free} · " + " | ".join(
                f"{o['kind']} {o['label']} ${o['cost']} p{o['p_win']:.2f} r{o.get('reach') or 0:.2f} L{o.get('long_mult') or 0:.2f}" for o in a.get("options", [])))
        for o in a.get("options", []):
            if o.get("unaffordable") or o["label"].startswith("pick "):
                continue
            kind, key = o["kind"], o.get("key")
            if (kind, key, o.get("sell_index")) in failed:
                continue
            try:
                acted = take(o, a, gs, sims, log)
            except RpcError as e:
                log.append(f"shop {o['label']} failed: {e}")
                failed.add((kind, key, o.get("sell_index")))
                # a step that went through before the refusal (the sale before a buy, which a
                # charged Invisible Joker's copy then refuses: card.lua `Card:calculate_joker`)
                # moved the jokers or the money: the options' places are stale, so analyse again
                live = rpc("gamestate")
                if live["money"] != gs["money"] or [j.get("key") for j in cards(live, "jokers")] != [j.get("key") for j in cards(gs, "jokers")]:
                    acted = "changed"
                    break
                gs = live
                continue
            if acted:
                took = o
                break
        why = why_not_tarot(top, gs) if top is not None and top["kind"] == "tarot" else None
        miss = f"shop: {top['kind']} {top['label']}{f' ({why})' if why else ''}" if top is not None and took is not top else None
        if miss and miss not in NOT_FOLLOWED[shop_start:]:  # once a shop
            NOT_FOLLOWED.append(miss)
        if not acted:
            break
    arrange_jokers(a, log)
    rpc("next_round")


def log_shop(a, gs, log):
    """One line per shop analysis: the money, the jokers and their stickers, and every option
    as ranked (its `rank`, `*` where `worth` takes it), so where the money went can be read back."""
    noise = a.get("shop_tie") or {}
    js = ", ".join(j["name"] + "".join(f" ({s})" for s in ("rental", "eternal", "debuff") if j.get(s)) for j in a.get("jokers", []))
    opts = " | ".join(f"{'*' if worth(o, a) else ''}{o['kind']} {o['label']} ${o['cost']} r{o.get('rank') or 0:.3f}"
                      f"{' (unaffordable)' if o.get('unaffordable') else ''}" for o in a.get("options", []))
    log.append(f"shop a{gs.get('ante_num')} r{gs.get('round_num')} ${gs['money']} tie {noise.get('tie')} "
               f"trouble {noise.get('trouble')} · jokers {js or '-'} · {opts}")


def arrange_jokers(a, log):
    """Puts the jokers in the order the advisor scores best (its `order`), as a player drags them:
    on leaving the shop and at each blind select, so each blind is played on the order the advice
    valued (outside a blind every analysis values the jokers as you'd arrange them, whatever
    order the game holds them in; in a blind, as they sit)."""
    order = a.get("order") or {}
    if order.get("is_current", True) or not order.get("order"):
        return
    names = [j.get("label") for j in cards(rpc("gamestate"), "jokers")]
    idx = []
    for n in order["order"]:
        i = next((k for k, x in enumerate(names) if x == n and k not in idx), None)
        if i is None:
            log.append(f"arrange: no joker named {n}")
            return
        idx.append(i)
    if len(idx) == len(names):
        rpc("rearrange", jokers=idx)
        log.append("arrange " + ", ".join(order["order"]))


def sell_for(o, gs):
    """Sells the joker the advisor names for this option (`sell_index` / `sell_key`), if any."""
    if o.get("sell_key") is None:
        return False
    if not sell_joker(gs, o):
        raise RpcError(f"no {o['sell_key']} at place {o.get('sell_index')} to sell")
    return True


def take(o, a, gs, sims, log):
    """Acts on one shop option if the bot can and it's worth it: what it did, or None."""
    kind, key = o["kind"], o.get("key")
    if not key and kind != "reroll" or not worth(o, a):
        return None
    if kind == "joker":
        i = index_of_key(gs, "shop", key)
        if i is None:
            return None
        if sell_for(o, gs):
            gs = rpc("gamestate")
            i = index_of_key(gs, "shop", key)
        rpc("buy", card=i)
        log.append(f"buy {o['label']}")
        return "joker"
    if kind == "planet" and held(o):
        use_held(gs, key)
        log.append(f"use {o['label']}")
        return "planet"
    if kind == "planet":
        i = index_of_key(gs, "shop", key)
        if i is None:
            return None
        rpc("buy", card=i)
        gs = rpc("gamestate")
        rpc("use", consumable=len(cards(gs, "consumables")) - 1)
        log.append(f"planet {o['label']}")
        return "planet"
    if kind == "voucher":
        i = index_of_key(gs, "vouchers", key)
        if i is None or "not valued" in o["note"]:
            return None
        rpc("buy", voucher=i)
        log.append(f"voucher {o['label']}")
        return "voucher"
    if kind == "tarot":
        if why_not_tarot(o, gs):
            return None
        if not held(o):
            i = index_of_key(gs, "shop", key)
            if i is None:
                return None
            rpc("buy", card=i)
            gs = rpc("gamestate")
            if key in NEEDS_HAND:
                FOR_BLIND.append(key)
                log.append(f"buy {o['label']} for the next blind")
                return "tarot"
        # bought, its money spent: a use the game refuses (no joker it can hit, ...) leaves it
        # held, and the shop is analysed again
        try:
            use_held(gs, key)
            log.append(f"use {o['label']}")
        except RpcError as e:
            log.append(f"use {o['label']} failed: {e}")
            if held(o):
                raise
        return "tarot"
    if kind == "pack" and (key.startswith("p_buffoon") or key.startswith("p_celestial")):
        i = index_of_key(gs, "packs", key)
        if i is None:
            return None
        rpc("buy", pack=i)
        log.append(f"pack {o['label']}")
        open_pack(sims, log)
        return "pack"
    if kind == "sell":
        if not sell_for(o, gs):
            return None
        log.append(o["label"])
        return "sell"
    if kind == "reroll":  # as the advisor ranks it: no count or money floor of the bot's own
        rpc("reroll")
        log.append("reroll")
        return "reroll"
    return None


SUITS = {"S": "♠", "H": "♥", "C": "♣", "D": "♦"}


def card_label(c):
    """A playing card from BalatroBot's gamestate as the advisor labels it (model.rs `Card::label`:
    `K♠ [glass] (foil) <Red seal>`), to find a Standard pack's pick, which has no key of its own."""
    v, m = c.get("value") or {}, c.get("modifier") or {}
    rank = {"T": "10"}.get(v.get("rank"), v.get("rank"))
    s = f"{rank}{SUITS.get(v.get('suit'), '?')}"
    if m.get("enhancement"):
        s += f" [{m['enhancement'].lower()}]"
    if m.get("edition"):
        s += f" ({m['edition'].lower()})"
    if m.get("seal"):
        s += f" <{m['seal'].capitalize()} seal>"
    return s


def pack_index(o, gs):
    """The pick's place in the open pack: by key, or a playing card by its label."""
    if o["kind"] != "card":
        return index_of_key(gs, "pack", o.get("key"))
    want = o["label"].removeprefix("pick ")
    return next((i for i, c in enumerate(cards(gs, "pack")) if card_label(c) == want), None)


def pick_from_pack(o, gs, log):
    """Takes one pick from the open pack, first selling the joker the advisor names for it (the
    game sells on any screen, card.lua `Card:can_sell_card`; BalatroBot's `sell` needs the
    bot's patched copy, see the docstring). None when taken, else why not, as `not_followed`
    words it."""
    i = pack_index(o, gs)
    if i is None:
        log.append(f"pack pick: no {o.get('key') or o['label']} in the pack")
        return "pack pick not in the pack"
    try:
        sold = sell_for(o, gs)
    except RpcError as e:
        log.append(f"pack pick's sale failed: {e}")
        return "pack pick needing a sale"
    if sold:
        log.append(f"sell {o['sell_key']} for the pick")
        gs = rpc("gamestate")
        i = pack_index(o, gs)
    try:
        rpc("pack", card=i)
    except RpcError as e:
        log.append(f"pack pick failed{' after its sale' if sold else ''}: {e}")
        return "pack pick refused after its sale" if sold else "pack pick refused"
    log.append(o["label"])
    return None


def open_pack(sims, log):
    """Takes the advisor's top pick while it is one the bot follows (a joker, a planet or a
    Standard pack's playing card), else the best one it follows ranked above skipping, else
    skips. Each time the top pick isn't the one taken, `not_followed` says why: a kind the bot doesn't take ("pack: tarot pick …"), a sale
    it couldn't make ("pack pick needing a sale: …"), a pick the game refused or not found. A pick
    refused after its sale went through skips the pack: the analysis's other sales were named
    for the board before it."""
    for _ in range(3):
        gs = rpc("gamestate")
        if gs["state"] != "SMODS_BOOSTER_OPENED":
            return
        a = analyze(sims, keep="pack")
        log.append(f"pack screen ${gs['money']} · " + " | ".join(f"{o['kind']} {o['label']} r{o.get('rank') or 0:.3f}" for o in a.get("options", [])))
        if VERBOSE:
            print(f"  pack screen {a.get('screen')} · " + " | ".join(f"{o['kind']} {o['label']} {o.get('key')}" for o in a.get("options", [])))
        took, why_not = None, None
        for o in a.get("options", []):
            if o["kind"] == "skip":
                break
            if not o["label"].startswith("pick "):
                continue
            if o["kind"] not in ("joker", "planet", "card"):
                why = "pack"  # a kind the bot doesn't take
            else:
                why = pick_from_pack(o, gs, log)
                if why is None:
                    took = o
                    break
            if why_not is None:
                why_not = f"{why}: {o['kind']} {o['label']}"
            if why == "pack pick refused after its sale":
                break  # the other picks' sales were named for the board before this one: skip
            if why != "pack":
                gs = rpc("gamestate")
                if gs["state"] != "SMODS_BOOSTER_OPENED":  # a refused pick still closed the pack
                    break
        if why_not is not None:
            NOT_FOLLOWED.append(why_not)
        if took is None:
            gs = rpc("gamestate")
            if gs["state"] == "SMODS_BOOSTER_OPENED":
                log.append("skip the pack")
                rpc("pack", skip=True)
            return


# Tags whose value in skip-or-play is a pick the bot doesn't make yet: a Mega Arcana or Spectral
# pack's best pick, targets included (advise.rs, the skip-or-play block). A tag the advisor
# doesn't value at all (`valued` false) isn't followed either: its skip is a verdict on what
# skipping costs alone, so unmodelled, and the bot plays.
TAGS_NOT_FOLLOWED = {"tag_charm": "a tarot pick", "tag_ethereal": "a spectral pick"}


def blind_select(args, log, skips, a=None):
    """Skips the blind on deck when the advisor's skip-or-play says skip (the blind's `skip`:
    `verdict` skip, play or close within 3%), else selects it; "close" plays. A skip whose tag is
    valued as a pick the bot can't make (`TAGS_NOT_FOLLOWED`) or not valued at all plays instead,
    counted in `not_followed`, as does a blind whose analysis failed. `a` is an analysis of this
    screen already made (the Ante 2 check's)."""
    if a is None:
        try:
            a = analyze(args.sims, keep="blind")
        except RuntimeError as e:
            log.append(f"blind: analysis failed, played: {e}")
            NOT_FOLLOWED.append("blind: analysis failed, played")
            rpc("select")
            return
    # the jokers as the advice valued them (outside a blind it values them as you'd arrange
    # them: gap 26), before the blind is played; a pack a skip tag opened at the last blind
    # select leaves its pick at the right end, with no shop to leave in between
    arrange_jokers(a, log)
    gs = rpc("gamestate")
    b = next((b for b in a.get("blinds", []) if b.get("state") == "Select" and b.get("skip")), None)
    if b is None:
        on_deck = next((x for x in a.get("blinds", []) if x.get("state") == "Select"), {})
        if on_deck.get("slot") != "Boss":
            log.append(f"blind: no skip-or-play for {on_deck.get('slot')}, played")
        rpc("select")
        return
    s, tag = b["skip"], b.get("skip_tag") or {}
    pv, sv = s["play_survive"] * s["play_next"] * s["play_long"], s["skip_survive"] * s["skip_next"] * s["skip_long"]
    log.append(f"blind a{gs.get('ante_num')} {b.get('slot')} {tag.get('name')}: {s['verdict']} · play {s['play_survive']:.2f}"
               f"×{s['play_next']:.2f}×{s['play_long']:.3f}={pv:.3f} skip {s['skip_survive']:.2f}×{s['skip_next']:.2f}"
               f"×{s['skip_long']:.3f}={sv:.3f} · {s['tag']}")
    if s["verdict"] != "skip":
        rpc("select")
        return
    why = TAGS_NOT_FOLLOWED.get(tag.get("key")) or (None if s.get("valued", True) else "its effect isn't valued")
    if why:
        NOT_FOLLOWED.append(f"blind: skip {b.get('slot')} for {tag.get('name')} ({why})")
        rpc("select")
        return
    rpc("skip")
    skips.append([gs.get("ante_num"), b.get("slot"), tag.get("key")])
    # a tag that opens a pack (Buffoon, Meteor, ...) does so a moment after the skip: the loop's
    # `settled` waits for it (the skip's tag events are queued before BalatroBot's `skip`
    # answers: button_callbacks.lua `skip_blind`'s immediate event runs ahead of skip.lua's check)


def settled():
    """The game's state once no blocking event is queued (the patched `save` waits for that, at
    most 5 s). At a blind select a skip tag's pack may still be on its way: each time the screen
    comes to the blind select (from the shop, a pack closing, a skip, a boss reroll) the game
    applies the next tag that opens one (game.lua `Game:update_blind_select`, button_callbacks.lua
    `end_consumeable`, `skip_blind`, `reroll_boss`), queued behind its 0.4 s flourish (tag.lua `Tag:yep`), so with two Double Tags
    held a Meteor Tag opens three packs in a row (BAVANG24). The bot read the blind select between
    them, and the next pack opened under its analysis ("select requires BLIND_SELECT")."""
    rpc("save", path=WIN_STATE)
    return rpc("gamestate")


def won(gs):
    """The game sets `G.GAME.won` at the end of the win ante's boss round whether it was beaten
    or not (state_events.lua `end_round`): a loss there is a game over with it set."""
    return bool(gs.get("won")) and gs["state"] != "GAME_OVER"


def windows_games():
    """The game's processes on Windows, {pid: resident MB}, from tasklist (its memory column is
    the working set, in K, with the locale's thousands separator)."""
    out = subprocess.run(["tasklist", "/FO", "CSV", "/NH", "/FI", "IMAGENAME eq Balatro.exe"],
                         capture_output=True, text=True, errors="replace").stdout
    games = {}
    for line in out.splitlines():
        f = [x.strip('"') for x in line.split('","')]
        if len(f) >= 5 and f[0].lower() == "balatro.exe":
            games[int(f[1])] = int(re.sub(r"\D", "", f[4]) or 0) // 1024
    return games


def game_pids():
    """The game's own process (Linux /proc: its executable, not the Steam and Proton wrappers
    that wait for it, nor a shell whose command line names it)"""
    if WINDOWS:
        return list(windows_games())
    pids = []
    for d in pathlib.Path("/proc").iterdir():
        try:
            argv0 = (d / "cmdline").read_bytes().split(b"\0")[0] if d.name.isdigit() else b""
            if argv0.endswith(b"Balatro.exe") or (NATIVE and argv0 == str(NATIVE_DIR / "love/bin/love").encode()):
                pids.append(int(d.name))
        except OSError:
            pass
    return pids


def game_rss_mb():
    """The game's resident memory (Linux, Windows; None elsewhere). The bot's modded game grows by about
    0.6-3.1 GB by a run's end from a fresh start (11.9 GB after 19 runs in one session, 2026-10-29)."""
    if WINDOWS:
        return next(iter(windows_games().values()), None)
    try:
        for pid in game_pids():
            kb = next(l for l in pathlib.Path(f"/proc/{pid}/status").read_text().splitlines() if l.startswith("VmRSS"))
            return int(kb.split()[1]) // 1024
    except (OSError, StopIteration, ValueError):
        pass
    return None


# The game launched with the bot's mods, as balatro-agent's tools/bot.sh `start` does (adapted
# from it: the launch only; bot.sh sets Profile 3 and backs up the owner's settings, which a
# restart in the bot's session leaves as they are). On Windows and natively botrun also starts and
# stops the game itself (`start_game`): there is no bot.sh there. Natively the game's save folder
# is LÖVE's (~/.local/share/love/Balatro), this machine's own, and the mods are NATIVE_DIR/BotMods.
if NATIVE:
    APPDATA = HOME / ".local/share/love/Balatro"
else:
    APPDATA = pathlib.Path(os.environ.get("APPDATA", HOME / "AppData/Roaming")) / "Balatro"
# The bot's own save folder, BalatroBot beside Balatro, once its profile is copied there: the bot's
# mods (BotMods/bav-bench) then switch the game to it, which Steam Cloud doesn't sync and the
# owner's game never reads. The mods stay in Balatro/BotMods (Windows) or NATIVE_DIR.
MODS_HOME = APPDATA


def own_save_folder():
    """On Windows, where the game is Steam's and Steam Cloud syncs its Balatro folder, botrun makes
    the bot's own folder the first time: the bot's Profile 3 (profile.jkr, meta.jkr) and a
    settings.jkr on profile 3 copied into BalatroBot. Natively there's no Steam Cloud to keep off,
    and on Linux with Steam bot.sh's machine has its copy already, so nothing is made there."""
    own = APPDATA.parent / "BalatroBot"
    if not WINDOWS or (own / "3" / "profile.jkr").exists() or not (APPDATA / "3" / "profile.jkr").exists():
        return
    (own / "3").mkdir(parents=True, exist_ok=True)
    for f in ("profile.jkr", "meta.jkr"):
        if (APPDATA / "3" / f).exists():
            shutil.copy2(APPDATA / "3" / f, own / "3" / f)
    if (APPDATA / "settings.jkr").exists():
        text = zlib.decompress((APPDATA / "settings.jkr").read_bytes(), -15).decode()
        text = re.sub(r'\["profile"\]=\d+', '["profile"]=3', text)
        c = zlib.compressobj(9, zlib.DEFLATED, -15)
        (own / "settings.jkr").write_bytes(c.compress(text.encode()) + c.flush())
    print(f"  (made the bot's own save folder {own}: its Profile 3 copied, off Steam Cloud)")


own_save_folder()
if (APPDATA.parent / "BalatroBot" / "3" / "profile.jkr").exists():
    APPDATA = APPDATA.parent / "BalatroBot"
if WINDOWS:
    BOT_MODS = str(MODS_HOME / "BotMods")
elif NATIVE:
    BOT_MODS = str(NATIVE_DIR / "BotMods")
else:
    BOT_MODS = r"C:\users\steamuser\AppData\Roaming\Balatro\BotMods"
BOT_PROFILE = 3
FRESH_MB = 1000  # a game just started holds ~0.6 GB


def windows_game_exe():
    """The installed game on Windows: BALATRO_EXE when it's an installed game (Lovely's
    version.dll beside it), else Steam's library from the registry."""
    exe = pathlib.Path(os.environ.get("BALATRO_EXE", ""))
    if exe.name.lower() == "balatro.exe" and (exe.parent / "version.dll").exists():
        return exe
    steam = pathlib.Path(r"C:\Program Files (x86)\Steam")
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Valve\Steam") as k:
            steam = pathlib.Path(winreg.QueryValueEx(k, "SteamPath")[0])
    except OSError:
        pass
    libraries = [steam]
    vdf = steam / "steamapps/libraryfolders.vdf"
    if vdf.exists():
        libraries += [pathlib.Path(p.replace("\\\\", "\\")) for p in re.findall(r'"path"\s+"([^"]+)"', vdf.read_text(errors="replace"))]
    for lib in libraries:
        exe = lib / "steamapps/common/Balatro/Balatro.exe"
        if exe.exists():
            return exe
    sys.exit("no installed Balatro.exe with Lovely's version.dll found (BALATRO_EXE, Steam's libraries)")


def settings_profile(profile=None):
    """The profile the game opens (settings.jkr's `profile`, the save's raw deflate as the game
    writes it, misc_functions.lua `compress_and_save`); with `profile`, sets it first and returns
    the one it replaced."""
    path = APPDATA / "settings.jkr"
    raw = zlib.decompress(path.read_bytes(), -15).decode()
    m = re.search(r'\["profile"\]=(\d+)', raw)
    was = int(m.group(1)) if m else 1
    if profile is not None and profile != was:
        raw = raw.replace(m.group(0), f'["profile"]={profile}') if m else raw.replace("return {", f'return {{["profile"]={profile},', 1)
        c = zlib.compressobj(9, zlib.DEFLATED, -15)
        path.write_bytes(c.compress(raw.encode()) + c.flush())
    return was


def close_game():
    """Closes the game (asked to quit, then forced after 20 s)."""
    import signal
    for pid in game_pids():
        if WINDOWS:
            subprocess.run(["taskkill", "/PID", str(pid)], capture_output=True)
        else:
            try:
                os.kill(pid, signal.SIGTERM)
            except OSError:
                pass
    t = time.monotonic()
    while game_pids() and time.monotonic() - t < 20:
        time.sleep(0.5)
    for pid in game_pids():
        if WINDOWS:
            subprocess.run(["taskkill", "/F", "/PID", str(pid)], capture_output=True)
        else:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
    time.sleep(3)


def launch_game():
    """Starts the game with the bot's mods. On Windows the game's own exe, which finds Steam
    through its steam_appid.txt (Lovely's version.dll beside it loads the --mod-dir), in a console
    of its own: Lovely shows its log there, as a normal launch does."""
    if WINDOWS:
        exe = windows_game_exe()
        subprocess.Popen([str(exe), "--mod-dir", BOT_MODS], cwd=str(exe.parent),
                         creationflags=subprocess.CREATE_NEW_CONSOLE | subprocess.CREATE_NEW_PROCESS_GROUP)
    elif NATIVE:
        exe = native_game_exe()
        # SDL's offscreen driver: no window at all (headless alone opens one at boot, then shrinks
        # it), and so no pointer over the game either; it still renders on the GPU
        env = dict(os.environ, LD_LIBRARY_PATH=str(NATIVE_DIR / "love/lib"),
                   LD_PRELOAD=str(NATIVE_DIR / "liblovely.so"), BALATROBOT_HEADLESS="1",
                   SDL_VIDEODRIVER="offscreen")
        if pathlib.Path("/dev/dxg").exists():
            env["GALLIUM_DRIVER"] = "d3d12"  # WSL: the GPU through Direct3D 12, not llvmpipe on the CPU
        wait_port_free()
        with open(NATIVE_DIR / "game.log", "w") as log:
            subprocess.Popen([str(NATIVE_DIR / "love/bin/love"), str(exe), "--mod-dir", BOT_MODS], cwd=str(exe.parent),
                             env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    else:
        # Lovely's console off under Proton: its window popped up at boot, and from 2026-11-02 10:55
        # Lovely hung writing to it (its log stopped at the 5th line, five launches) until disabled
        subprocess.Popen(["steam", "-applaunch", "2379780", "--mod-dir", BOT_MODS, "--disable-console"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)


def native_game_exe():
    exe = pathlib.Path(os.environ.get("BALATRO_EXE", ""))
    if exe.name != "Balatro.exe" or not exe.exists():
        sys.exit("set BALATRO_EXE to the game file (vault advisor-second-machine.md)")
    return exe


def wait_port_free(s=90):
    """Linux: waits until no socket holds BalatroBot's port. A game started while the last one's
    connections linger finds it taken and runs without its API ("Failed to bind", seen 2026-11-02)."""
    port = ":%04X " % int(API.rsplit(":", 1)[1])
    t = time.monotonic()
    while time.monotonic() - t < s:
        held = any(port in line.split()[1] + " " for f in ("/proc/net/tcp", "/proc/net/tcp6")
                   if os.path.exists(f) for line in open(f).read().splitlines()[1:])
        if not held:
            return
        time.sleep(1)


def wait_for_bot(s=180):
    t = time.monotonic()
    while time.monotonic() - t < s:
        try:
            rpc("health")
            return time.monotonic() - t
        except Exception:
            time.sleep(2)
    return None


def check_native_luajit():
    """Natively, the LuaJIT the game loads (or will) must be the Windows game's own 2.0.5 (the
    docstring): the one in native/love/lib, and any a running game has mapped."""
    libs = {str(NATIVE_DIR / "love/lib/libluajit-5.1.so.2")}
    for pid in game_pids():
        try:
            maps = pathlib.Path(f"/proc/{pid}/maps").read_text()
        except OSError:
            continue
        libs |= {line.split(None, 5)[5] for line in maps.splitlines()
                 if len(line.split(None, 5)) == 6 and "libluajit" in line}
    for lib in libs:
        try:
            ok = b"LuaJIT 2.0.5" in pathlib.Path(lib).read_bytes()
        except OSError:  # replaced on disk since the game loaded it (" (deleted)")
            ok = False
        if not ok:
            sys.exit(f"{lib} isn't LuaJIT 2.0.5, the Windows game's: with LÖVE's own the runs aren't "
                     "the main PC's (the docstring); the vault's advisor-second-machine.md says how to build "
                     "it (close a game started with another first)")


def start_game():
    """What bot.sh `start` does on Linux with Steam, on Windows and natively: the game on the bot's
    profile with its mods, waited for until BalatroBot answers. Returns the profile settings.jkr
    had, for `stop_game`."""
    if NATIVE:
        needs = (pathlib.Path(BOT_MODS) / "Balatrobot", NATIVE_DIR / "liblovely.so", APPDATA / str(BOT_PROFILE))
        note = "advisor-second-machine.md"
    else:
        needs = (pathlib.Path(BOT_MODS) / "Balatrobot", windows_game_exe().parent / "version.dll")
        note = "advisor-windows-laptop.md"
    for need in needs:
        if not need.exists():
            sys.exit(f"missing {need}: the vault's {note} says how the bot is set up")
    if game_pids():
        sys.exit("Balatro is running without BalatroBot answering: close it first (the bot starts its own)")
    was = settings_profile(BOT_PROFILE)
    launch_game()
    s = wait_for_bot()
    if s is None:
        close_game()
        settings_profile(was)
        sys.exit("BalatroBot didn't answer within 3 minutes of starting the game")
    print(f"  (game started on profile {BOT_PROFILE} in {s:.0f} s)")
    return was


def stop_game(was):
    """What bot.sh `stop` does: closes the game and gives the game back the profile it had."""
    close_game()
    settings_profile(was)
    print(f"  (game closed, profile {was} restored)")


STEAM_LOG = pathlib.Path.home() / ".steam/steam/logs/console_log.txt"
STEAM_WAIT_S = 3600  # how long a restart waits on Steam (offline, or a dialog) before the series stops
STEAM_PASSING = ("ShowInterstitials", "CreatingProcess")  # prompts every launch logs and passes at once
STEAM_HELD_S = 20  # a prompt still open after this long holds the launch (a dialog, a shader update)


def steam_log_tail(size=2_000_000):
    """The last `size` bytes of Steam's console log (Linux) as lines; [] where there's none."""
    try:
        with open(STEAM_LOG, "rb") as f:
            f.seek(0, 2)
            f.seek(max(0, f.tell() - size))
            return f.read().decode("utf-8", "replace").splitlines()
    except OSError:
        return []


def steam_log_since(t0, lines=None):
    """Steam's console log lines written at or after the wall-clock time `t0`. Each line starts
    "[YYYY-MM-DD HH:MM:SS]", local time, so they compare as strings."""
    since = time.strftime("[%Y-%m-%d %H:%M:%S]", time.localtime(t0))
    return [l for l in (steam_log_tail() if lines is None else lines) if l[:21] >= since]


def steam_offline():
    """Whether Steam's latest logon line is a failure: after a suspend it can take minutes to log on
    again ("LogonFailure No Connection"), and a game launched meanwhile stops at its Cloud sync
    ("syncfailed") and waits for a click. A logon logs "post-logon"."""
    for l in reversed(steam_log_since(time.time() - 6 * 3600)):
        if "post-logon" in l:
            return False
        if "LogonFailure" in l:
            return True
    return False


def steam_held(launched, lines=None):
    """What Steam holds the game's launch on, if anything: the launch's last "waiting for user
    response to <stage> <detail>" since `launched` with no "continues with user response" after it,
    open for STEAM_HELD_S, and not one of the prompts every launch passes (STEAM_PASSING). None
    where nothing holds it."""
    last, at = None, None
    for l in steam_log_since(launched, lines):
        if "2379780" not in l:
            continue
        if "waiting for user response to" in l:
            last, at = l.split("waiting for user response to", 1)[1].strip(), l[1:20]
        elif "continues with user response" in l:
            last = None
    if not last or last.split()[0] in STEAM_PASSING:
        return None
    opened = time.mktime(time.strptime(at, "%Y-%m-%d %H:%M:%S"))
    return last if time.time() - opened >= STEAM_HELD_S else None


def restart_game():
    """Closes the bot's game and starts it again (Linux with Steam, Windows, natively), so a run
    starts from a fresh game: the modded game grows as it plays. False where it can't. On Linux
    with Steam it waits for Steam to be logged on before launching; when Steam holds the launch on
    a prompt (a Cloud sync that failed or conflicts, a shader update) it says which and waits, up to
    STEAM_WAIT_S, launching again every 2 minutes: a dialog cancelled lets the next launch through,
    one still open ignores it."""
    if not (WINDOWS or NATIVE or (sys.platform.startswith("linux") and shutil.which("steam"))):
        return False
    close_game()
    if WINDOWS or NATIVE:
        settings_profile(BOT_PROFILE)  # the game saves its settings as it quits
    else:
        t = time.monotonic()
        if steam_offline():
            print("  (Steam is offline, e.g. just after a suspend: waiting for it to log on)")
            while steam_offline() and time.monotonic() - t < STEAM_WAIT_S:
                time.sleep(10)
            if steam_offline():
                sys.exit(f"Steam didn't log on within {STEAM_WAIT_S // 60} minutes")
            print(f"  (Steam logged on after {time.monotonic() - t:.0f} s)")
    launched = time.time()
    launch_game()
    t = relaunched = time.monotonic()
    told = None
    while True:
        try:
            rpc("health")
            print(f"  (game restarted in {time.monotonic() - t:.0f} s)")
            return True
        except Exception:
            time.sleep(2)
        held = None if WINDOWS or NATIVE else steam_held(launched)
        if held and held != told:
            told = held
            print(f"  (Steam holds the game's launch on: {held}. In Steam, answer it: a Cloud sync that failed"
                  " (Retry, or cancel it), a conflict (Keep local), a shader update (wait, or Skip);"
                  " the series goes on once the game answers)", flush=True)
        if told and time.monotonic() - relaunched > 120:
            relaunched = time.monotonic()
            launch_game()  # ignored while the dialog is open; goes through once it's cancelled
        if time.monotonic() - t > (STEAM_WAIT_S if told else 180):
            break
    sys.exit("the game didn't come back after a restart" + (f" (Steam held it on {told} for an hour)" if told else " within 3 minutes"))


def commit():
    """The checkout's commit, to tie a run to the code (the installed advisor is whatever
    `cargo install` last built: keep them the same)."""
    here = str(pathlib.Path(__file__).resolve().parent)
    out = subprocess.run(["git", "-C", here, "rev-parse", "--short", "HEAD"], capture_output=True, text=True)
    # (a checkout with changes to tracked files is marked: its runs aren't the commit's)
    dirty = subprocess.run(["git", "-C", here, "status", "--porcelain", "-uno"], capture_output=True, text=True).stdout.strip()
    return (out.stdout.strip() + ("+" if dirty else "")) or None


def one_run(args, n, seed=None):
    global ANALYSIS_S, KEEP_DIR
    ANALYSIS_S = 0.0
    NOT_FOLLOWED.clear()
    FOR_BLIND.clear()
    rpc("menu")
    seed = seed or f"BOT{int(time.time()) % 100000:05d}{n}"
    # a seed played again (paired runs, --repeat) keeps its states in a folder of its own
    KEEP_DIR = OUT / "shops" / seed
    k = 1
    while KEEP_DIR.exists():
        k += 1
        KEEP_DIR = OUT / "shops" / f"{seed}-{k}"
    KEEP_DIR.mkdir(parents=True)
    rpc("start", deck=args.deck, stake=args.stake, seed=seed)
    for _ in range(60):  # the last run's game-over screen can outlast the start
        if rpc("gamestate")["state"] == "BLIND_SELECT":
            break
        time.sleep(0.5)
    # monotonic: a suspended PC pauses the clock, so a run that spans a suspend keeps its true time
    log, best_ante, t0, shop_money = [], 1, time.monotonic(), []
    checked, restarted, ante2_reach, skips = False, False, None, []
    while True:
        gs = rpc("gamestate")
        st = gs["state"]
        best_ante = max(best_ante, gs.get("ante_num", 1))
        if st == "GAME_OVER" or won(gs):
            break
        try:
            if st == "BLIND_SELECT":
                gs = settled()  # a tag's pack still to open opens first
                st = gs["state"]
            if st == "BLIND_SELECT" and gs.get("ante_num") == 2 and not checked:
                # Gold Stake habit: a weak start isn't worth playing out. After Ante 1, if the
                # board reaches less than this share of a plain Ante 3 boss, start over.
                checked = True
                a = analyze(args.sims, keep="blind")
                hz = next((r for r in a.get("rounds", []) if r.get("horizon")), None)
                # logged on every run, so a restart threshold can be judged afterwards on runs played out
                ante2_reach = hz.get("reach") if hz else None
                if hz and args.restart_below and hz.get("reach", 1) < args.restart_below:
                    log.append(f"restart: next-ante reach {hz['reach']:.2f}")
                    restarted = True
                    break
                blind_select(args, log, skips, a)
            elif st == "BLIND_SELECT":
                blind_select(args, log, skips)
            elif st == "SELECTING_HAND":
                play_hand(args.sims, log)
            elif st == "ROUND_EVAL":
                rpc("cash_out")
            elif st == "SHOP":
                shop_money.append([gs.get("ante_num"), gs["money"]])
                shop(args.sims, log)
            elif st == "SMODS_BOOSTER_OPENED":
                open_pack(args.sims, log)
            else:
                time.sleep(0.5)
        except (RpcError, RuntimeError) as e:
            log.append(f"error in {st}: {e}")
            time.sleep(1)
            if sum(1 for l in log if l.startswith("error")) > 20:
                break
    gs = rpc("gamestate")
    result = {
        "strategy": args.strategy, "deck": args.deck, "stake": args.stake, "seed": seed,
        "won": won(gs), "ante": best_ante, "restarted": restarted,
        "ended": "won" if won(gs) else "game over" if gs["state"] == "GAME_OVER" else "errors" if not restarted else "restart",
        "errors": sum(1 for l in log if l.startswith("error")), "shop_money": shop_money, "ante2_reach": ante2_reach, "skips": skips, "label": args.label, "advisor_seed": ADVISOR_SEED, "commit": commit(),
        "not_followed": NOT_FOLLOWED[:],
        "jokers": [j.get("label") for j in cards(gs, "jokers")],
        "game_rss_mb": game_rss_mb(), "minutes": round((time.monotonic() - t0) / 60, 1), "analysis_minutes": round(ANALYSIS_S / 60, 1), "log": log, "states": str(KEEP_DIR), "time": int(time.time()),
    }
    with open(OUT / "runs.jsonl", "a") as f:
        f.write(json.dumps(result) + "\n")
    print(f"run {n + 1}: {'restarted after Ante 1' if restarted else f'ante {best_ante}'}{' WON' if result['won'] else ''} · {result['minutes']} min · {', '.join(result['jokers'])}")
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--runs", type=int, default=1)
    p.add_argument("--seeds", default="", help="comma-separated game seeds to play (paired runs); overrides --runs")
    p.add_argument("--repeat", type=int, default=1, help="runs per seed in --seeds")
    p.add_argument("--advisor-seed", type=int, default=42, help="the advisor's simulation seed (another one is a placebo change)")
    p.add_argument("--label", default="", help="a tag stored with each run (e.g. the commit)")
    p.add_argument("--deck", default="RED")
    p.add_argument("--stake", default="WHITE")
    p.add_argument("--sims", type=int, default=100, help="advisor round simulations (fewer = faster)")
    p.add_argument("--strategy", default="advisor")
    p.add_argument("-v", "--verbose", action="store_true", help="print every hand")
    p.add_argument("--restart-game", choices=["each", "never"], default="each",
                   help="each: restart the game before a run once it holds more than a fresh one (it grows as it plays; Linux with Steam, Windows, native)")
    p.add_argument("--max-game-mb", type=int, default=8000,
                   help="where the game isn't restarted: stop between runs once its memory passes this, printing the seeds to resume with")
    p.add_argument("--restart-below", type=float, default=0.0,
                   help="after Ante 1, start over if the board reaches less than this share of the next plain boss (e.g. 0.5)")
    args = p.parse_args()
    global VERBOSE, ADVISOR_SEED
    VERBOSE = args.verbose
    ADVISOR_SEED = args.advisor_seed
    OUT.mkdir(parents=True, exist_ok=True)
    started_from = None  # Windows, native: the profile settings.jkr had before botrun started the game
    if NATIVE:
        check_native_luajit()
    try:
        rpc("health")
    except Exception:
        if not (WINDOWS or NATIVE):
            sys.exit("BalatroBot isn't answering on 127.0.0.1:12346: start the game with balatro-agent's tools/bot.sh start")
        started_from = start_game()
    try:
        seeds = [s.strip() for s in args.seeds.split(",") if s.strip()]
        todo = [s for s in seeds for _ in range(args.repeat)] or [None] * args.runs
        for n, seed in enumerate(todo):
            rss = game_rss_mb()
            if rss and rss > FRESH_MB and not (args.restart_game == "each" and restart_game()) and rss > args.max_game_mb:
                left = ",".join(todo[n:]) if seeds else f"{len(todo) - n} more runs"
                sys.exit(f"the game holds {rss} MB (over --max-game-mb {args.max_game_mb}): restart it, then resume with --seeds {left}")
            one_run(args, n, seed)
    finally:
        if started_from is not None:
            stop_game(started_from)


if __name__ == "__main__":
    main()
