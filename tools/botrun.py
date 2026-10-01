#!/usr/bin/env python3
"""Plays Balatro runs through BalatroBot with the advisor making every choice, and logs
how far each run got. For strategy experiments on Profile 3 (start the game with
balatro-agent's tools/bot.sh first).

    tools/botrun.py --runs 5 --stake WHITE

Strategy "advisor": play the advisor's best play; in the shop take the advisor's top
option it can act on (jokers, planets, Buffoon/Celestial packs, rerolls) while it's worth
it, else leave. Tarots that need target cards aren't used yet.
"""
import argparse
import json
import pathlib
import subprocess
import sys
import time
import urllib.request

API = "http://127.0.0.1:12346"
VERBOSE = False
HOME = pathlib.Path.home()
OUT = HOME / ".local/share/balatro-advisor/bot"
STATE = OUT / "state.jkr"
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


def analyze(sims, quick=False):
    rpc("save", path=WIN_STATE)
    out = subprocess.run(
        ["balatro-advisor", "analyze", "--file", str(STATE), "--sims", str(sims)] + (["--quick"] if quick else []),
        capture_output=True, text=True, timeout=300,
    )
    if out.returncode != 0:
        raise RuntimeError(out.stderr.strip()[-300:])
    return json.loads(out.stdout)


def cards(gs, area):
    return (gs.get(area) or {}).get("cards") or []


def index_of_key(gs, area, key):
    for i, c in enumerate(cards(gs, area)):
        if c.get("key") == key:
            return i
    return None


def sell_named(gs, name):
    for i, j in enumerate(cards(gs, "jokers")):
        if j.get("label") == name:
            rpc("sell", joker=i)
            return True
    return False


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
    if bp["action"] == "discard" and gs["round"]["discards_left"] > 0:
        rpc("discard", cards=idx)
    else:
        rpc("play", cards=idx)


def worth(o):
    """The advisor ranks options; take one only when it's an improvement: better by Ante 8,
    a better win chance now, or (with a free joker slot) a higher score this round."""
    if (o.get("long_mult") or 1.0) > 1.0 or o.get("p_win", 0) > o.get("_now", 0) + 0.02:
        return True
    return o["kind"] == "joker" and o.get("_free_slot") and (o.get("reach") or 0) > o.get("_reach", 0) * 1.03


def shop(sims, log):
    rerolls = 0
    for _ in range(8):
        a = analyze(sims)
        gs = rpc("gamestate")
        money = gs["money"]
        rnd = a["rounds"][a["options_round"]] if a.get("rounds") else {}
        now, reach = rnd.get("p_win", 0), rnd.get("reach", 0)
        free = len(cards(gs, "jokers")) < (gs.get("jokers") or {}).get("limit", 5)
        acted = False
        if VERBOSE:
            print(f"  shop ${money} · now {now:.2f} reach {reach:.2f} free slot {free} · " + " | ".join(
                f"{o['kind']} {o['label']} ${o['cost']} p{o['p_win']:.2f} r{o.get('reach') or 0:.2f} L{o.get('long_mult') or 0:.2f}" for o in a.get("options", [])))
        for o in a.get("options", []):
            o["_now"], o["_reach"], o["_free_slot"] = now, reach, free
            if o.get("unaffordable") or o["label"].endswith("(you have it)") or o["label"].startswith("pick "):
                continue
            kind, key = o["kind"], o.get("key")
            if kind == "joker" and key and worth(o):
                i = index_of_key(gs, "shop", key)
                if i is None:
                    continue
                note = o.get("note", "")
                if "sell " in note and " for it" in note:
                    sell_named(gs, note.split("sell ", 1)[1].split(" for it", 1)[0])
                    gs = rpc("gamestate")
                    i = index_of_key(gs, "shop", key)
                rpc("buy", card=i)
                log.append(f"buy {o['label']}")
                acted = True
            elif kind == "planet" and key and worth(o):
                i = index_of_key(gs, "shop", key)
                if i is None:
                    continue
                rpc("buy", card=i)
                gs = rpc("gamestate")
                rpc("use", consumable=len(cards(gs, "consumables")) - 1)
                log.append(f"planet {o['label']}")
                acted = True
            elif kind == "pack" and key and (key.startswith("p_buffoon") or key.startswith("p_celestial")) and worth(o):
                i = index_of_key(gs, "packs", key)
                if i is None:
                    continue
                rpc("buy", pack=i)
                log.append(f"pack {o['label']}")
                open_pack(sims, log)
                acted = True
            elif kind == "reroll" and rerolls < 2 and money - o["cost"] >= 5 and worth(o):
                rpc("reroll")
                rerolls += 1
                log.append("reroll")
                acted = True
            if acted:
                break
        if not acted:
            break
    rpc("next_round")


def open_pack(sims, log):
    for _ in range(3):
        gs = rpc("gamestate")
        if gs["state"] != "SMODS_BOOSTER_OPENED":
            return
        a = analyze(sims)
        took = False
        for o in a.get("options", []):
            if not o["label"].startswith("pick ") or o["kind"] not in ("joker", "planet"):
                continue
            i = index_of_key(gs, "pack", o.get("key"))
            if i is None:
                continue
            try:
                rpc("pack", card=i)
                log.append(o["label"])
                took = True
            except RpcError as e:
                log.append(f"pack pick failed: {e}")
            break
        if not took:
            rpc("pack", skip=True)
            return


def one_run(args, n):
    rpc("menu")
    seed = f"BOT{int(time.time()) % 100000:05d}{n}"
    rpc("start", deck=args.deck, stake=args.stake, seed=seed)
    log, best_ante, t0 = [], 1, time.time()
    while True:
        gs = rpc("gamestate")
        st = gs["state"]
        best_ante = max(best_ante, gs.get("ante_num", 1))
        if st == "GAME_OVER" or gs.get("won"):
            break
        try:
            if st == "BLIND_SELECT":
                rpc("select")
            elif st == "SELECTING_HAND":
                play_hand(args.sims)
            elif st == "ROUND_EVAL":
                rpc("cash_out")
            elif st == "SHOP":
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
        "won": bool(gs.get("won")), "ante": best_ante,
        "jokers": [j.get("label") for j in cards(gs, "jokers")],
        "minutes": round((time.time() - t0) / 60, 1), "log": log[-60:], "time": int(time.time()),
    }
    with open(OUT / "runs.jsonl", "a") as f:
        f.write(json.dumps(result) + "\n")
    print(f"run {n + 1}: ante {best_ante}{' WON' if result['won'] else ''} · {result['minutes']} min · {', '.join(result['jokers'])}")
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--runs", type=int, default=1)
    p.add_argument("--deck", default="RED")
    p.add_argument("--stake", default="WHITE")
    p.add_argument("--sims", type=int, default=100, help="advisor round simulations (fewer = faster)")
    p.add_argument("--strategy", default="advisor")
    p.add_argument("-v", "--verbose", action="store_true", help="print every hand")
    args = p.parse_args()
    global VERBOSE
    VERBOSE = args.verbose
    OUT.mkdir(parents=True, exist_ok=True)
    try:
        rpc("health")
    except Exception:
        sys.exit("BalatroBot isn't answering on 127.0.0.1:12346: start the game with balatro-agent's tools/bot.sh start")
    for n in range(args.runs):
        one_run(args, n)


if __name__ == "__main__":
    main()
