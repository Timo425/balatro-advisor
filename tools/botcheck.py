#!/usr/bin/env python3
"""The bot's snapshot check: re-runs the bot's own choices (tools/botrun.py) on the moments a run
kept, with an older botrun against the one in this checkout, and lists every moment where the bot
would now act differently. No game and no new analysis: each kept moment carries what BalatroBot
showed the bot (`.gamestate.json`) and the advisor's answer (`.advice.json`), saved by botrun's
`keep_state` since 2026-11-02; older runs have neither and are passed over.

    tools/botcheck.py                      # every kept run, HEAD's botrun against this checkout's
    tools/botcheck.py --base 2b66933 ~/.local/share/balatro-advisor/bot/shops/BAVANG24-6

What it replays: a shop (`shop`, until the bot's first action: later actions are kept moments of
their own), an open pack (`open_pack`) and a blind select (`blind_select`). Each answer from the
game is the moment's gamestate, changed only as far as needed for the bot to go on (a card bought
moves to your consumables; the screen moves on after a pick or a select). So a game's refusal (full
slots, a sale it won't make) isn't seen here, nor anything in a blind: those are the live run's.
What the bot did is the list of calls it made to the game and what it counted as not followed."""
import argparse
import copy
import importlib.util
import json
import pathlib
import subprocess
import sys
import tempfile
import types

REPO = pathlib.Path(__file__).resolve().parent.parent
SHOPS = pathlib.Path.home() / ".local/share/balatro-advisor/bot/shops"
LEAVE = {"options": [{"kind": "leave", "label": "next round", "cost": 0, "note": "", "rank": 1.0}], "shop_tie": None}


def load(path, name):
    """botrun as a module of its own, from `path` (it reads data/game.json beside its folder)."""
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def base_botrun(rev, into):
    """botrun.py and the game data at `rev`, written where `load` finds them together."""
    for f in ("tools/botrun.py", "data/game.json"):
        out = into / f
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(subprocess.run(["git", "-C", str(REPO), "show", f"{rev}:{f}"], capture_output=True, check=True).stdout)
    return into / "tools/botrun.py"


def replay(mod, screen, gs, advice):
    """What the bot does on this moment: (its calls to the game, what it counted as not followed)."""
    gs = copy.deepcopy(gs)
    calls, first = [], [True]

    def rpc(method, **params):
        if method == "gamestate":
            return gs
        calls.append([method, params])
        if method == "buy" and "card" in params:
            shop = gs.get("shop") or {"cards": []}
            c = shop["cards"].pop(params["card"])
            if c.get("set") in ("TAROT", "PLANET", "SPECTRAL"):
                gs.setdefault("consumables", {"cards": []})["cards"].append(c)
        elif method in ("pack", "select", "skip"):
            gs["state"] = "DONE"
        return gs

    def analyze(sims, quick=False, keep=None):
        out = advice if first[0] else LEAVE
        first[0] = False
        return out

    mod.rpc, mod.analyze = rpc, analyze
    mod.NOT_FOLLOWED.clear()
    if hasattr(mod, "FOR_BLIND"):
        mod.FOR_BLIND.clear()
    log = []
    fn = {"shop": "shop", "pack": "open_pack", "blind": "blind_select"}[screen]
    if not hasattr(mod, fn):  # an older botrun that didn't make this choice
        return [["(no " + fn + ")", {}]], []
    if screen == "shop":
        mod.shop(100, log)
    elif screen == "pack":
        mod.open_pack(100, log)
    elif screen == "blind":
        mod.blind_select(types.SimpleNamespace(sims=100), log, [], advice)
    return calls, mod.NOT_FOLLOWED[:]


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("dirs", nargs="*", help="kept-run folders (default: every one under bot/shops)")
    p.add_argument("--base", default="HEAD", help="the botrun to compare against (a git revision)")
    args = p.parse_args()
    dirs = [pathlib.Path(d) for d in args.dirs] or sorted(d for d in SHOPS.iterdir() if d.is_dir())
    with tempfile.TemporaryDirectory() as tmp:
        old = load(base_botrun(args.base, pathlib.Path(tmp)), "botrun_base")
        new = load(REPO / "tools/botrun.py", "botrun_new")
        seen = moved = 0
        for d in dirs:
            for adv in sorted(d.glob("*.advice.json")):
                stem = adv.name.removesuffix(".advice.json")
                screen = stem.rsplit("-", 1)[-1]
                gsf = d / f"{stem}.gamestate.json"
                if screen not in ("shop", "pack", "blind") or not gsf.exists():
                    continue
                gs, advice = json.loads(gsf.read_text()), json.loads(adv.read_text())
                a, b = replay(old, screen, gs, advice), replay(new, screen, gs, advice)
                seen += 1
                if a != b:
                    moved += 1
                    print(f"{d.name}/{stem}:\n  before {a[0]} {a[1]}\n  after  {b[0]} {b[1]}")
    print(f"{seen} kept moments replayed, {moved} where the bot acts differently ({args.base} → this checkout)")
    if seen == 0:
        print("(no kept moment has its gamestate and advice: runs before 2026-11-02 don't)", file=sys.stderr)


if __name__ == "__main__":
    main()
