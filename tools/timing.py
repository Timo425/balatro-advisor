#!/usr/bin/env python3
"""How long the advice takes, old build against new, on the same saved states (CLAUDE.md step 4).

Runs `balatro-advisor analyze` of two builds on a fixed set of the owner's saved states
(`tools/timing-states.txt`: shops on the full pass, hands in a blind on the quick pass the live
page shows first), one after the other on each state and `--repeat` times over, so both builds
meet the same load. A state's time is its fastest run. Prints each group's total, old against
new, the states that got slower on every repeat, and where the new build stands against the
budget the states file sets for this machine (the owner's numbers).

    cargo build --release                      in the base's worktree and in this checkout
    tools/timing.py --old <base worktree>/target/release/balatro-advisor \\
                    --new target/release/balatro-advisor
    tools/timing.py --new target/release/balatro-advisor      one build's times alone
    ... --slow                                 the states marked slow too (minutes each),
                                               totalled on their own

Exit status: 0 the set is no slower than the limit and inside the budget; 1 a group's total
grew by more than `--limit` percent (default 10: one build's totals repeat within 0.4-2.3% on
this set; a starting line, the owner's to move), or is over this machine's budget, or a state
failed or ran out of time on every run of a build; 2 a listed state isn't on this machine, or
`--new` was built before the code it is to judge changed. Over the limit isn't a failure by
itself: the change says what the time buys (CLAUDE.md step 4).

The states are the owner's saved history (`BAV_HISTORY`, else
`~/.local/share/balatro-advisor/history`), which the live page prunes after 90 days: each is
copied once to `timing-states/` beside the history, which nothing prunes, and read from there.
A heavy job: one at a time per PC.
"""
import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
GROUPS = (("quick", "a hand in a blind, quick pass"), ("full", "a shop, full pass"), ("slow", "the slow states"))


def data_dir():
    base = os.environ.get("XDG_DATA_HOME") or os.path.join(os.path.expanduser("~"), ".local", "share")
    return os.path.join(base, "balatro-advisor")


def read_states(path):
    """The states `quick|full <seed>/<file> [slow]` as (group, pass, state), a state marked slow
    in the group "slow", and the budgets `budget <group> <seconds> <machine>` for this machine
    as {group: seconds}"""
    states, budgets = [], {}
    for line in open(path, encoding="utf-8"):
        f = line.split("#")[0].split()
        if len(f) >= 4 and f[0] == "budget" and f[3] == platform.node():
            budgets[f[1]] = float(f[2])
        elif len(f) >= 2 and f[0] in ("quick", "full"):
            states.append(("slow" if "slow" in f[2:] else f[0], f[0], f[1]))
    return states, budgets


def kept(state):
    """The state's file: its copy in `timing-states/`, made from the history the first time"""
    keep = os.path.join(data_dir(), "timing-states", state.replace("/", os.sep))
    if not os.path.exists(keep):
        hist = os.path.join(os.environ.get("BAV_HISTORY") or os.path.join(data_dir(), "history"), state.replace("/", os.sep))
        if not os.path.exists(hist):
            return None
        os.makedirs(os.path.dirname(keep), exist_ok=True)
        shutil.copy2(hist, keep)
    return keep


def newer_source(binary):
    """A file the library is built from that changed after `binary` was built: the first one"""
    built = os.path.getmtime(binary)
    for root in (os.path.join(REPO, "crates", "core", "src"), os.path.join(REPO, "crates", "cli", "src"), os.path.join(REPO, "data")):
        for d, _, files in os.walk(root):
            for f in files:
                if os.path.getmtime(os.path.join(d, f)) > built:
                    return os.path.relpath(os.path.join(d, f), REPO)
    return None


def run(binary, state, quick, timeout, said):
    """Seconds one analysis took, or None when it failed or ran out of time. What the build
    says on stderr is shown once (a warning of its own, such as one about its age)."""
    cmd = [binary, "analyze", "--file", state, "--json"] + (["--quick"] if quick else [])
    if os.name != "nt":
        cmd = ["nice", "-n", "19"] + cmd
    t = time.time()
    try:
        r = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    except subprocess.TimeoutExpired:
        return None
    took = time.time() - t
    err = r.stderr.decode("utf-8", "replace").strip()
    if err and binary not in said:
        said.add(binary)
        print(f"   {binary} says: {err.splitlines()[0][:200]}")
    try:
        json.loads(r.stdout)
    except ValueError:
        return None
    return took if r.returncode == 0 else None


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--old", help="the base's build (omit for one build's times alone)")
    ap.add_argument("--new", required=True, help="the build to judge")
    ap.add_argument("--states", default=os.path.join(HERE, "timing-states.txt"))
    ap.add_argument("--repeat", type=int, default=2, help="runs of each build on each state (default 2)")
    ap.add_argument("--limit", type=float, default=10, help="percent a group's total may grow (default 10)")
    ap.add_argument("--timeout", type=float, default=900, help="seconds one analysis may take (default 900)")
    ap.add_argument("--slow", action="store_true", help="the states marked slow too")
    ap.add_argument("--any-build", action="store_true", help="time --new though the code changed after it was built (an installed or older build, on purpose)")
    ap.add_argument("--out", help="write the times as JSON here")
    a = ap.parse_args()
    for b in (a.old, a.new):
        if b and not os.path.exists(b):
            sys.exit(f"{b}: no such build (cargo build --release)")
    stale = None if a.any_build else newer_source(a.new)
    if stale:
        print(f"{a.new} was built before {stale} changed: cargo build --release (or --any-build to time it as it is)")
        return 2
    listed, budgets = read_states(a.states)
    # every listed state gets its kept copy, timed this run or not: the history is pruned
    states = [(g, p, s, kept(s)) for g, p, s in listed]
    states = [x for x in states if a.slow or x[0] != "slow"]
    missing = [s for _, _, s, f in states if f is None]
    if missing:
        print(f"not on this machine (neither kept in {os.path.join(data_dir(), 'timing-states')} nor in the saved history): {', '.join(missing)}")
        return 2
    if hasattr(os, "getloadavg") and os.getloadavg()[0] > 2:
        print(f"load {os.getloadavg()[0]:.1f}: something else is running, the times will be noisy; the budget is "
              "for a quiet machine, so run again when it is before calling a group over")
    builds = [("old", a.old), ("new", a.new)] if a.old else [("new", a.new)]
    rows, said = [], set()
    for g, p, s, f in states:
        times = {name: [] for name, _ in builds}
        for _ in range(a.repeat):
            for name, b in builds:
                times[name].append(run(b, f, p == "quick", a.timeout, said))
        rows.append(dict(state=s, group=g, **times))
        cells = "  ".join(f"{name} " + "/".join("failed" if t is None else f"{t:.1f}" for t in ts) for name, ts in times.items())
        print(f"{p:<5} {s}  {cells} s", flush=True)
    if a.out:
        with open(a.out, "w", encoding="utf-8") as out:
            json.dump(dict(old=a.old, new=a.new, machine=platform.node(), rows=rows), out, indent=1)

    def best(ts):
        return min((t for t in ts if t is not None), default=None)

    failed = [r for r in rows if any(best(r[name]) is None for name, _ in builds)]
    for r in failed:
        which = ", ".join(name for name, _ in builds if best(r[name]) is None)
        print(f"FAILED or over {a.timeout:.0f} s on every run of {which}, left out of the totals: {r['state']}")
    bad = bool(failed)
    print()
    for g, label in GROUPS:
        mine = [r for r in rows if r["group"] == g and r not in failed]
        if not mine:
            continue
        new = sum(best(r["new"]) for r in mine)
        budget = budgets.get(g)
        over = budget is not None and new > budget
        bad |= over
        against = "" if budget is None else f"; this machine's budget {budget:.0f} s{': OVER' if over else ''}"
        if not a.old:
            print(f"{label}: {len(mine)} states, {new:.1f} s in all, the slowest {max(best(r['new']) for r in mine):.1f} s{against}")
            continue
        old = sum(best(r["old"]) for r in mine)
        pct = 100 * (new - old) / old
        bad |= pct > a.limit
        print(f"{label}: {len(mine)} states, {old:.1f} -> {new:.1f} s ({pct:+.0f}%{', OVER the limit' if pct > a.limit else ''}){against}")
        # slower on every repeat: the slowest old run still faster than the fastest new one
        for r in mine:
            olds, news = [t for t in r["old"] if t is not None], [t for t in r["new"] if t is not None]
            if max(olds) * (1 + a.limit / 100) < min(news):
                print(f"   slower on every run: {r['state']}  {best(r['old']):.1f} -> {best(r['new']):.1f} s")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
