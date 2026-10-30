#!/usr/bin/env python3
"""Plays Balatro runs through BalatroBot with the advisor making every choice, and logs
how far each run got. For strategy experiments on Profile 3 (start the game with
balatro-agent's tools/bot.sh first).

    tools/botrun.py --runs 5 --stake WHITE
    tools/botrun.py --seeds BAV00001,BAV00002 --stake GOLD   # paired: the same seeds before and after

The advisor's simulations are seeded (`analyze --seed`, 42), so the same code on the same game
seed plays the same run. `--advisor-seed 43` is a placebo: a change that does nothing but flips
the picks noise decides, so its paired difference is the noise a real change is judged against.

Strategy "advisor": play the advisor's best play; in the shop take the advisor's top
option it can act on while it's worth it (clear of keeping the money by more than the
ranking's noise: each option's `rank` against the advisor's `shop_tie`), else leave. A joker
bought into full slots sells the one the advisor's option names by place and key
(`sell_index`, `sell_key`).

A run measures the advisor only where the bot follows it. Read runs.jsonl's `not_followed`
(each time the advisor's top choice was one the bot can't take) before reading a run as a
verdict on the advice. As of 2026-10-24 the bot:
- follows: plays and discards (Best play's quick pass, 100 simulations where the live page
  runs 300); joker buys, with the sale the advisor names; planets; rerolls (2 a shop);
  Buffoon and Celestial packs and their joker and planet picks, with the sale the advisor
  names; the jokers' order after each shop (the advisor's `order`, one for the board: no
  order is advised per hand, docs/design.md gap 37).
- doesn't: tarots and spectrals (bought or in packs), vouchers, Arcana, Spectral and Standard
  packs, "use X first" in a blind, skipping a blind (always plays it; the advisor isn't asked).
  Counted in `not_followed`, except skipping a blind. Since 2026-10-25 it also sells a joker on
  its own when the advisor's "sell X" option clears keeping the money (`worth`).
- needs: the bot profile (3) with every joker unlocked, as the owner's (the advisor's shop pool
  assumes every joker unlocked, gap 38); Steamodded's guard against saving in an open pack
  lifted in the bot's own copy (BotMods/Steamodded/lovely/fixes.toml), or the advisor never
  sees a pack's cards.
Since 2026-10-29 every state the advisor analysed in a shop or an open pack, and every state an
analysis failed on (`error`), is kept (`bot/shops/<seed>/a<ante>-r<round>-<n>-<screen>.jkr`;
`analyze --file` replays it) and the run's log is whole: each shop analysis logs the money, the jokers' stickers and every option ranked.
Every run before 2026-10-24 (none has a `commit`) is marked `void` in runs.jsonl: every pack
skipped, the jokers never arranged, "won" read from a flag the game also sets on a loss, and
before 2026-10-23 35 jokers locked and an early advisor. Don't read them as evidence.

The game is restarted before a run once it holds more than a fresh one (`--restart-game`, Linux
with Steam: it keeps 0.3-0.9 GB a run, measured 2026-10-29); elsewhere a series stops between runs
past `--max-game-mb` and prints the seeds to resume with. The runs are seeded, so a restart
between them changes nothing.

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
import pathlib
import re
import shutil
import subprocess
import sys
import time
import urllib.request
import zlib

API = "http://127.0.0.1:12346"
VERBOSE = False
ANALYSIS_S = 0.0  # the advisor's share of a run's time
NOT_FOLLOWED = []  # the advisor's top choices the bot couldn't take, this run
HOME = pathlib.Path.home()
OUT = HOME / ".local/share/balatro-advisor/bot"
STATE = OUT / "state.jkr"
KEEP_DIR = None  # this run's kept states (bot/shops/<seed>/): every shop and pack the advisor analysed
WIN_STATE = "Z:" + str(STATE)  # the game runs under Proton: Z: is the Linux root


class RpcError(Exception):
    pass


_id = 0


def rpc(method, **params):
    global _id
    _id += 1
    body = json.dumps({"jsonrpc": "2.0", "method": method, "params": params, "id": _id}).encode()
    req = urllib.request.Request(API, body, {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as r:
        data = json.load(r)
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
    so a run's shops can be replayed with `analyze --file` afterwards: state.jkr is overwritten."""
    if KEEP_DIR is None:
        return
    gs = rpc("gamestate")
    n = sum(1 for _ in KEEP_DIR.iterdir())
    (KEEP_DIR / f"a{gs.get('ante_num')}-r{gs.get('round_num')}-{n:03d}-{screen}.jkr").write_bytes(STATE.read_bytes())


def analyze(sims, quick=False, keep=None):
    global ANALYSIS_S
    rpc("save", path=WIN_STATE)
    v = vanilla_state(STATE)
    if keep:
        keep_state(keep)
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
    return json.loads(out.stdout)


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


def play_hand(sims):
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
    if bp["action"] == "discard" and gs["round"]["discards_left"] > 0:
        rpc("discard", cards=idx)
    else:
        rpc("play", cards=idx)


def worth(o, a):
    """The advisor ranks every option against keeping the money ("next round" / "skip the
    pack"). Worth taking: ranked above that and clear of it by more than the ranking's noise
    (each option's `rank`, the ranking's own measure; ties within `tie` of the top's, the
    advisor's `shop_tie`). When the round is in trouble the ranking goes by the score reached
    (`trouble`), and ranked above keeping the money is enough."""
    options, noise = a.get("options", []), a.get("shop_tie")
    keep = next((i for i, x in enumerate(options) if x["kind"] in ("leave", "skip")), len(options))
    if options.index(o) >= keep:
        return False
    k = options[keep] if keep < len(options) else None
    if k is None or noise is None or noise.get("trouble") or o.get("rank") is None or k.get("rank") is None:
        return True
    top = max(x.get("rank") or 0.0 for x in options)
    return o["rank"] - k["rank"] > noise["tie"] * top


def shop(sims, log):
    rerolls = 0
    failed = set()  # options the game refused this shop: not tried again
    shop_start = len(NOT_FOLLOWED)
    for _ in range(8):
        a = analyze(sims, keep="shop")
        gs = rpc("gamestate")
        money = gs["money"]
        log_shop(a, gs, log)
        rnd = a["rounds"][a["options_round"]] if a.get("rounds") else {}
        now, reach = rnd.get("p_win", 0), rnd.get("reach", 0)
        free = len(cards(gs, "jokers")) < (gs.get("jokers") or {}).get("limit", 5)
        acted, took = False, None
        top = next((o for o in a.get("options", []) if not o.get("unaffordable") and not o["label"].endswith("(you have it)")
                    and not o["label"].startswith("pick ") and worth(o, a)), None)
        if VERBOSE:
            print(f"  shop ${money} · now {now:.2f} reach {reach:.2f} free slot {free} · " + " | ".join(
                f"{o['kind']} {o['label']} ${o['cost']} p{o['p_win']:.2f} r{o.get('reach') or 0:.2f} L{o.get('long_mult') or 0:.2f}" for o in a.get("options", [])))
        for o in a.get("options", []):
            if o.get("unaffordable") or o["label"].endswith("(you have it)") or o["label"].startswith("pick "):
                continue
            kind, key = o["kind"], o.get("key")
            if (kind, key, o.get("sell_index")) in failed:
                continue
            try:
                acted = take(o, a, gs, money, rerolls, sims, log)
            except RpcError as e:
                log.append(f"shop {o['label']} failed: {e}")
                failed.add((kind, key, o.get("sell_index")))
                continue
            if acted == "reroll":
                rerolls += 1
            if acted:
                took = o
                break
        miss = f"shop: {top['kind']} {top['label']}" if top is not None and took is not top else None
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
    """Puts the jokers in the order the advisor scores best (its `order`), as a player drags them."""
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


def take(o, a, gs, money, rerolls, sims, log):
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
    if kind == "planet":
        i = index_of_key(gs, "shop", key)
        if i is None:
            return None
        rpc("buy", card=i)
        gs = rpc("gamestate")
        rpc("use", consumable=len(cards(gs, "consumables")) - 1)
        log.append(f"planet {o['label']}")
        return "planet"
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
    if kind == "reroll" and rerolls < 2 and money - o["cost"] >= 5:
        rpc("reroll")
        log.append("reroll")
        return "reroll"
    return None


def open_pack(sims, log):
    for _ in range(3):
        gs = rpc("gamestate")
        if gs["state"] != "SMODS_BOOSTER_OPENED":
            return
        a = analyze(sims, keep="pack")
        log.append(f"pack screen ${gs['money']} · " + " | ".join(f"{o['kind']} {o['label']} r{o.get('rank') or 0:.3f}" for o in a.get("options", [])))
        if VERBOSE:
            print(f"  pack screen {a.get('screen')} · " + " | ".join(f"{o['kind']} {o['label']} {o.get('key')}" for o in a.get("options", [])))
        took = False
        top = next((o for o in a.get("options", []) if o["kind"] == "skip" or o["label"].startswith("pick ")), None)
        if top and top["kind"] not in ("skip", "joker", "planet"):
            NOT_FOLLOWED.append(f"pack: {top['kind']} {top['label']}")
        for o in a.get("options", []):
            if o["kind"] == "skip":
                break
            if not o["label"].startswith("pick ") or o["kind"] not in ("joker", "planet"):
                continue
            i = index_of_key(gs, "pack", o.get("key"))
            if i is None:
                continue
            try:
                if sell_for(o, gs):
                    gs = rpc("gamestate")
                    i = index_of_key(gs, "pack", o.get("key"))
                rpc("pack", card=i)
                log.append(o["label"])
                took = True
            except RpcError as e:
                log.append(f"pack pick failed: {e}")
            break
        if not took:
            log.append("skip the pack")
            rpc("pack", skip=True)
            return


def won(gs):
    """The game sets `G.GAME.won` at the end of the win ante's boss round whether it was beaten
    or not (state_events.lua `end_round`): a loss there is a game over with it set."""
    return bool(gs.get("won")) and gs["state"] != "GAME_OVER"


def game_pids():
    """The game's own process (Linux /proc: its executable, not the Steam and Proton wrappers
    that wait for it, nor a shell whose command line names it)"""
    pids = []
    for d in pathlib.Path("/proc").iterdir():
        try:
            if d.name.isdigit() and (d / "cmdline").read_bytes().split(b"\0")[0].endswith(b"Balatro.exe"):
                pids.append(int(d.name))
        except OSError:
            pass
    return pids


def game_rss_mb():
    """The game's resident memory (Linux; None elsewhere). The bot's modded game grows by about
    0.3-0.9 GB a run (11.9 GB after 19 runs, 2026-10-29)."""
    try:
        for pid in game_pids():
            kb = next(l for l in pathlib.Path(f"/proc/{pid}/status").read_text().splitlines() if l.startswith("VmRSS"))
            return int(kb.split()[1]) // 1024
    except (OSError, StopIteration, ValueError):
        pass
    return None


# The game launched with the bot's mods, as balatro-agent's tools/bot.sh `start` does (adapted
# from it: the launch only; bot.sh sets Profile 3 and backs up the owner's settings, which a
# restart in the bot's session leaves as they are)
BOT_MODS = r"C:\users\steamuser\AppData\Roaming\Balatro\BotMods"
FRESH_MB = 1000  # a game just started holds ~0.6 GB


def restart_game():
    """Closes the bot's game and starts it again (Linux, Steam), so a run starts from a fresh game:
    the modded game keeps 0.3-0.9 GB a run. False where it can't (no Steam, not Linux)."""
    if not sys.platform.startswith("linux") or not shutil.which("steam"):
        return False
    import os
    import signal
    for pid in game_pids():
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
    t = time.monotonic()
    while game_pids() and time.monotonic() - t < 20:
        time.sleep(0.5)
    for pid in game_pids():
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass
    time.sleep(3)
    subprocess.Popen(["steam", "-applaunch", "2379780", "--mod-dir", BOT_MODS], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    t = time.monotonic()
    while time.monotonic() - t < 180:
        try:
            rpc("health")
            print(f"  (game restarted in {time.monotonic() - t:.0f} s)")
            return True
        except Exception:
            time.sleep(2)
    sys.exit("the game didn't come back within 3 minutes after a restart")


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
    checked, restarted, ante2_reach = False, False, None
    while True:
        gs = rpc("gamestate")
        st = gs["state"]
        best_ante = max(best_ante, gs.get("ante_num", 1))
        if st == "GAME_OVER" or won(gs):
            break
        try:
            if st == "BLIND_SELECT" and gs.get("ante_num") == 2 and not checked:
                # Gold Stake habit: a weak start isn't worth playing out. After Ante 1, if the
                # board reaches less than this share of a plain Ante 3 boss, start over.
                checked = True
                a = analyze(args.sims)
                hz = next((r for r in a.get("rounds", []) if r.get("horizon")), None)
                # logged on every run, so a restart threshold can be judged afterwards on runs played out
                ante2_reach = hz.get("reach") if hz else None
                if hz and args.restart_below and hz.get("reach", 1) < args.restart_below:
                    log.append(f"restart: next-ante reach {hz['reach']:.2f}")
                    restarted = True
                    break
                rpc("select")
            elif st == "BLIND_SELECT":
                rpc("select")
            elif st == "SELECTING_HAND":
                play_hand(args.sims)
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
        "errors": sum(1 for l in log if l.startswith("error")), "shop_money": shop_money, "ante2_reach": ante2_reach, "label": args.label, "advisor_seed": ADVISOR_SEED, "commit": commit(),
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
                   help="each: restart the game before a run once it holds more than a fresh one (it keeps 0.3-0.9 GB a run; Linux with Steam)")
    p.add_argument("--max-game-mb", type=int, default=8000,
                   help="where the game isn't restarted: stop between runs once its memory passes this, printing the seeds to resume with")
    p.add_argument("--restart-below", type=float, default=0.0,
                   help="after Ante 1, start over if the board reaches less than this share of the next plain boss (e.g. 0.5)")
    args = p.parse_args()
    global VERBOSE, ADVISOR_SEED
    VERBOSE = args.verbose
    ADVISOR_SEED = args.advisor_seed
    OUT.mkdir(parents=True, exist_ok=True)
    try:
        rpc("health")
    except Exception:
        sys.exit("BalatroBot isn't answering on 127.0.0.1:12346: start the game with balatro-agent's tools/bot.sh start")
    seeds = [s.strip() for s in args.seeds.split(",") if s.strip()]
    todo = [s for s in seeds for _ in range(args.repeat)] or [None] * args.runs
    for n, seed in enumerate(todo):
        rss = game_rss_mb()
        if rss and rss > FRESH_MB and not (args.restart_game == "each" and restart_game()) and rss > args.max_game_mb:
            left = ",".join(todo[n:]) if seeds else f"{len(todo) - n} more runs"
            sys.exit(f"the game holds {rss} MB (over --max-game-mb {args.max_game_mb}): restart it, then resume with --seeds {left}")
        one_run(args, n, seed)


if __name__ == "__main__":
    main()
