#!/usr/bin/env python3
"""Which check runs were on the code you're about to commit (CLAUDE.md step 4).

Every check (`cargo test --release --test <x> -- --ignored <check>`: the snapshot, the history
picks and judge, the player bench, the parity tier, the search reference, the shop judges) logs
its start and end to `balatro-advisor-checks.tsv` in this machine's temp folder
(`crates/core/tests/common/stamp.rs`), and refuses to start on a test program older than the
code. This lists the runs of the last day, or those writing to the given output folders, each
with what it ran on:

    this code      the library's inputs (crates/core/src, data/game.json) as this checkout holds
                   them now
    older code     they changed since the run started: the files changed after it are named
    <commit> ...   a run in another checkout (a worktree of the base): its commit, and the
                   commits on this checkout's HEAD since that touch the library's inputs, which
                   a "before" run doesn't have

and whether it finished (done, failed, or no end logged: running, or killed, its output folder
perhaps still holding an earlier run's files).

    tools/checks.py                      the last day's runs
    tools/checks.py --hours 3
    tools/checks.py /tmp/after ...       the runs into these folders; exits 1 unless, in each,
                                         every run in this checkout since its last full one
                                         (without BAV_ONLY or BAV_SEEDS) finished on this code
"""
import argparse
import os
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATE = os.path.join(REPO, "crates", "core")
INPUTS = ["src", "../../data/game.json"]  # stamp.rs INPUTS
FILTERS = ("BAV_ONLY=", "BAV_SEEDS=")


def temp_dir():
    """Rust's `std::env::temp_dir`: TMP, TEMP on Windows; TMPDIR elsewhere"""
    if os.name == "nt":
        return os.environ.get("TMP") or os.environ.get("TEMP") or os.environ.get("USERPROFILE", ".")
    return os.environ.get("TMPDIR") or "/tmp"


def input_files():
    """(path from the crate, path) of every input file, in stamp.rs's order"""
    out = []
    for inp in INPUTS:
        p = os.path.normpath(os.path.join(CRATE, inp))
        if os.path.isdir(p):
            for d, _, files in os.walk(p):
                for f in files:
                    full = os.path.join(d, f)
                    out.append((inp + "/" + os.path.relpath(full, p).replace(os.sep, "/"), full))
        elif os.path.exists(p):
            out.append((inp, p))
    return sorted(out)


def source_hash():
    """stamp.rs `source_hash` of this checkout"""
    h = 0xCBF29CE484222325
    mask = (1 << 64) - 1

    def eat(h, data):
        for b in data:
            h = ((h ^ b) * 0x100000001B3) & mask
        return h

    for name, path in input_files():
        with open(path, "rb") as f:
            text = f.read().replace(b"\r\n", b"\n")
        h = eat(eat(eat(eat(h, name.encode()), b"\0"), text), b"\0")
    return f"{h:016x}"


def same_dir(a, b):
    return os.path.normcase(os.path.normpath(os.path.abspath(a))) == os.path.normcase(os.path.normpath(os.path.abspath(b)))


def git(*args):
    r = subprocess.run(["git", "-C", REPO, *args], capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


def read_log(path):
    starts, ends = [], {}
    try:
        lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
    except FileNotFoundError:
        return starts
    for line in lines:
        f = line.split("\t")
        if len(f) >= 4 and f[1] == "end":
            ends[f[2]] = f[3]
        elif len(f) >= 10 and f[1] == "start":
            starts.append(dict(when=int(f[0]), id=f[2], check=f[3], out=f[4], hash=f[5], crate=f[6], commit=f[7], state=f[8], settings=f[9]))
    for s in starts:
        s["end"] = ends.get(s["id"], "no end logged")
    return starts


def verdict(run, now_hash, head):
    if same_dir(run["crate"], CRATE):
        if run["hash"] == now_hash:
            return "this code", True
        changed = [n for n, p in input_files() if os.path.getmtime(p) > run["when"]]
        names = ", ".join(n.removeprefix("src/") for n in changed[:4]) + (f" and {len(changed) - 4} more" if len(changed) > 4 else "")
        return f"older code (changed since: {names or 'a file removed or restored'})", False
    where = f"{run['commit']} {'as committed' if run['state'] == 'clean' else 'with changes'}, in {run['crate']}"
    if head and run["commit"] != "?":
        since = git("log", "--oneline", f"{run['commit']}..HEAD", "--", "crates/core/src", "data")
        if since:
            n = len(since.splitlines())
            where += f"; HEAD has {n} commit{'s' if n > 1 else ''} since that change the library"
    return where, False


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dirs", nargs="*", help="output folders (BAV_SNAPSHOT_DIR, BAV_PICKS, ...) to check")
    ap.add_argument("--hours", type=float, default=24, help="how far back to list (default 24)")
    ap.add_argument("--log", default=os.path.join(temp_dir(), "balatro-advisor-checks.tsv"))
    a = ap.parse_args()
    runs = read_log(a.log)
    if not runs:
        print(f"no check runs logged in {a.log}")
        return 1 if a.dirs else 0
    now_hash, head = source_hash(), git("rev-parse", "--short=10", "HEAD")
    print(f"this checkout: HEAD {head}, the library's inputs {now_hash}")
    since = time.time() - a.hours * 3600
    shown = [r for r in runs if r["when"] >= since and (not a.dirs or any(r["out"] and same_dir(r["out"], d) for d in a.dirs))]
    for r in shown:
        what, _ = verdict(r, now_hash, head)
        settings = " ".join(s for s in r["settings"].split() if not (r["out"] and s.split("=", 1)[-1] and same_dir(s.split("=", 1)[-1], r["out"])))
        print(f"{time.strftime('%m-%d %H:%M', time.localtime(r['when']))}  {r['check']:<26} {r['end']:<14} {what}")
        print(f"{'':13} -> {r['out'] or '(no output folder)'}{'  ' + settings if settings else ''}")
    bad = []
    for d in a.dirs:
        mine = [r for r in shown if r["out"] and same_dir(r["out"], d) and same_dir(r["crate"], CRATE)]
        full = [i for i, r in enumerate(mine) if not any(s.startswith(FILTERS) for s in r["settings"].split())]
        mine = mine[full[-1]:] if full else mine
        if not mine:
            print(f"{d}: no run in this checkout in the last {a.hours:g} hours")
            continue
        stale = [r for r in mine if r["end"] != "done" or verdict(r, now_hash, head)[0] != "this code"]
        if stale:
            bad.append(d)
            print(f"{d}: NOT all on this code and finished ({len(stale)} of {len(mine)} runs)")
        else:
            print(f"{d}: on this code ({len(mine)} run{'s' if len(mine) > 1 else ''}, finished)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
