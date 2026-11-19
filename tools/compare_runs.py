#!/usr/bin/env python3
"""Two runs of one seed side by side, shop by shop: the owner's run (the live page's saved states,
`history/<seed>/`) against the bot's (`bot/shops/<seed>[-n]/`, tools/botrun.py). Each shop visit
is keyed by the blind beaten before it (ante, slot), so a skip doesn't shift the rows; a skipped
blind shows its tag. Per visit and side: money in and out, jokers got (bought or from a pack) and
sold, vouchers bought, packs opened, rerolls (counted from the reroll price's rises), tarots used,
hand levels gained in all and the top hand, skips; the board after it.

    tools/compare_runs.py A7LD1T91                                       # the bot's dir bot/shops/A7LD1T91
    tools/compare_runs.py A7LD1T91 --bot ~/.local/share/balatro-advisor/bot/shops/A7LD1T91-2 --json

How to read it: CLAUDE.md "Test a card by what it changes", level 3. The two compare choice by
choice only at their first different choice (marked) and where a later shop shows the same offer
on the same board and money (marked "comparable"). Past the first difference the rows point where
to look (levels, money, skips that drift apart), each to be read on the bot's own saved moments;
they don't say who played better. Reads the saves in place (`balatro-advisor state --json`)."""
import argparse
import collections
import json
import pathlib
import re
import subprocess

DATA = pathlib.Path.home() / ".local/share/balatro-advisor"
ED = {"e_foil": "foil", "e_holo": "holo", "e_polychrome": "poly", "e_negative": "neg",
      "foil": "foil", "holo": "holo", "polychrome": "poly", "negative": "neg"}
SLOTS = {"Small": 0, "Big": 1, "Boss": 2}


def state(p):
    out = subprocess.run(["balatro-advisor", "state", "--file", str(p), "--json"], capture_output=True, text=True)
    try:
        return json.loads(out.stdout)
    except ValueError:
        return None


def screen_of(name):
    n = name.lower()
    for word, screen in (("shop", "shop"), ("pack", "pack"), ("blind", "blind")):
        if word in n:
            return screen
    return "other"


def joker(j):
    tags = [ED.get(j["edition"], str(j["edition"]))] if j.get("edition") else []
    tags += [t for t in ("eternal", "rental", "perishable") if j.get(t)]
    return j["name"] + (f" ({', '.join(tags)})" if tags else "")


def ident(j):
    """One joker through a visit: its key and the hands played when it was made (an edition
    change keeps it the same joker)."""
    return j["key"], (j.get("ability") or {}).get("hands_played_at_create")


def key_of(s):
    """The blind beaten before this shop: (ante, slot). After a boss the shop already shows the
    next ante, every blind Upcoming."""
    blinds = s.get("blinds", [])
    beaten = [b["slot"] for b in blinds if b["state"] == "Defeated"]
    if not beaten or all(b["state"] == "Upcoming" for b in blinds):
        return s["ante"] - 1, "Boss"
    return s["ante"], beaten[-1]


def offer(s):
    sh = s.get("shop") or {}
    return {"jokers": [joker(j) + f" ${j['cost']}" for j in sh.get("jokers", [])],
            "cards": [c["name"] for c in sh.get("other_cards", [])],
            "packs": [b["name"] for b in sh.get("boosters", [])],
            "vouchers": [v["name"] for v in sh.get("vouchers", [])]}


def vname(k):
    return k.removeprefix("v_").replace("_", " ").title()


def visit(states, exit_):
    """One shop visit: its shop and pack screens in order, then the state after it."""
    entry = states[0][1]
    every = [s for _, s in states] + [exit_]
    names, seen = {}, set()
    for s in every:
        for j in s["jokers"]:
            names.setdefault(ident(j), joker(j))
            seen.add(ident(j))
    first = {ident(j) for j in entry["jokers"]}
    last = {ident(j): joker(j) for j in exit_["jokers"]}
    rerolls, prev = 0, None
    for sc, s in states:
        cost = (s.get("shop") or {}).get("reroll_cost") if sc == "shop" else None
        if prev is not None and cost is not None and cost > prev:
            rerolls += cost - prev  # the reroll price rises $1 a reroll
        prev = cost if cost is not None else prev
    packs = sum(1 for k, (sc, _) in enumerate(states) if sc == "pack" and (k == 0 or states[k - 1][0] != "pack"))
    levels = exit_.get("hand_levels", {})
    return {
        "key": key_of(entry),
        "money_in": entry["dollars"], "money_out": exit_["dollars"],
        "got": sorted(names[i] for i in seen - first),
        "sold": sorted(names[i] for i in seen - set(last)),
        "changed": [f"{names[i]} -> {last[i]}" for i in first if i in last and names[i] != last[i]],
        "jokers": [joker(j) for j in exit_["jokers"]],
        "jokers_in": sorted(joker(j) for j in entry["jokers"]),
        "vouchers_new": sorted(vname(v) for v in set(exit_.get("vouchers", [])) - set(entry.get("vouchers", []))),
        "held": sorted(c.get("name", c.get("key")) for c in exit_["consumables"]),
        "tarots_used": exit_.get("tarots_used"),
        "levels": sum(v["level"] - 1 for v in levels.values()),
        "top": max(((v["level"], h) for h, v in levels.items()), default=(1, "-")),
        "skips": exit_.get("skips"),
        "packs": packs, "rerolls": rerolls,
        "offer": offer(entry),
    }


def side(files):
    """files: one run's saved states in order. Returns its shop visits, its end and its skips."""
    seq = []
    for f in files:
        sc = screen_of(f.name)
        if sc != "other" and (s := state(f)):
            seq.append((sc, s))
    last = state(files[-1]) if files else None
    skipped = {}
    for s in [s for _, s in seq] + ([last] if last else []):
        blinds = s.get("blinds", [])
        # once its boss is beaten the ante moves on before the blinds reset: that set is the last ante's
        own = s["ante"] - 1 if any(b["slot"] == "Boss" and b["state"] == "Defeated" for b in blinds) else s["ante"]
        for b in blinds:
            if b["state"] == "Skipped":
                skipped[(own, b["slot"])] = (b.get("skip_tag") or "?").replace("tag_", "")
    visits, i = [], 0
    while i < len(seq):
        if seq[i][0] != "shop":
            i += 1
            continue
        j = i
        while j < len(seq) and seq[j][0] in ("shop", "pack"):
            j += 1
        exit_ = seq[j][1] if j < len(seq) else (last or seq[j - 1][1])
        visits.append(visit(seq[i:j], exit_))
        i = j
    end = None
    if last:
        b = last.get("current_blind") or {}
        end = {"ante": last.get("ante"), "won": bool(last.get("won")), "screen": last.get("screen"),
               "blind": b.get("name"), "scored": b.get("scored"), "target": b.get("target")}
    return visits, end, skipped


def owner_files(seed):
    return sorted((DATA / "history" / seed).glob("*.jkr"), key=lambda p: int(p.name.split("-")[0]))


def bot_files(d):
    return sorted(pathlib.Path(d).glob("*.jkr"), key=lambda p: int(re.match(r"a\d+-r\d+-(\d+)", p.name).group(1)))


def row(who, v):
    top = f"{v['top'][1]} L{v['top'][0]}" if v["top"][0] > 1 else "all L1"
    print(f"  {who}: ${v['money_in']:.0f} -> ${v['money_out']:.0f}"
          f" | got {', '.join(v['got']) or '-'} | sold {', '.join(v['sold']) or '-'}"
          f"{' | voucher ' + ', '.join(v['vouchers_new']) if v['vouchers_new'] else ''}"
          f"{' | changed ' + ', '.join(v['changed']) if v['changed'] else ''}"
          f" | packs {v['packs']} rerolls {v['rerolls']} | tarots used {v['tarots_used']}"
          f" | levels +{v['levels']} ({top}) | skips {v['skips']}")
    print(f"       jokers: {', '.join(v['jokers']) or '-'}; held: {', '.join(v['held']) or '-'}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("seed")
    ap.add_argument("--bot", help="the bot's saved run (default bot/shops/<seed>)")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()
    you, you_end, you_sk = side(owner_files(a.seed))
    bot, bot_end, bot_sk = side(bot_files(a.bot or DATA / "bot" / "shops" / a.seed))
    if a.json:
        print(json.dumps({"seed": a.seed, "you": you, "bot": bot, "you_end": you_end, "bot_end": bot_end,
                          "you_skipped": [[k[0], k[1], t] for k, t in you_sk.items()],
                          "bot_skipped": [[k[0], k[1], t] for k, t in bot_sk.items()]}, indent=1))
        return
    by = collections.defaultdict(dict)
    for who, vs in (("you", you), ("bot", bot)):
        for v in vs:
            by[tuple(v["key"])][who] = v
    for k in set(you_sk) | set(bot_sk):
        by[k]
    first = None
    for k in sorted(by, key=lambda k: (k[0], SLOTS[k[1]])):
        y, b = by[k].get("you"), by[k].get("bot")
        print(f"\n== Ante {k[0]} after the {k[1]} blind")
        if first is None and (k in you_sk) != (k in bot_sk) and (y or k in you_sk) and (b or k in bot_sk):
            first = k
            print("  ^^ first difference: one side skipped this blind")
        for who, v, sk in (("you", y, you_sk), ("bot", b, bot_sk)):
            if v:
                row(who, v)
            else:
                print(f"  {who}: " + (f"skipped this blind (took the {sk[k]} tag)" if k in sk else "(no shop: the run had ended)"))
        if y and b and first is not None and y["offer"] == b["offer"]:
            same = y["jokers_in"] == b["jokers_in"] and y["money_in"] == b["money_in"]
            print("  == the same shop on both sides" + (", the same board and money: comparable" if same
                  else ", on another board or money: not the same choice"))
        if y and b and first is None:
            mine = lambda v: (v["got"], v["sold"], v["vouchers_new"], v["packs"], v["rerolls"])
            if mine(y) != mine(b):
                first = k
                print("  ^^ first shop where the choices differ")
                print(f"     offer you: {y['offer']}")
                print(f"     offer bot: {b['offer']}")
    print(f"\nend: you {you_end}\n     bot {bot_end}")


if __name__ == "__main__":
    main()
