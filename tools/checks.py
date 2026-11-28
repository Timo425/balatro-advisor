#!/usr/bin/env python3
"""Which check runs were on the code you're about to commit (CLAUDE.md step 4).

Every check (`cargo test --release --test <x> -- --ignored <check>`: the snapshot, the history
picks and judge, the player bench, the parity tier, the search reference, the shop judges) logs
its start and end to `balatro-advisor-checks.tsv` in this machine's temp folder
(`crates/core/tests/common/stamp.rs`), and refuses to start on a test program older than the
code. This lists the runs of the last day, or those writing to the given output folders, each
with what it ran on:

    this code      the library's inputs (crates/core/src, data/game.json) as this checkout holds
                   them now, and none of the test's own sources changed since the run started
    older code     they changed since: the files changed after the run started are named
    <commit> ...   a run in another checkout (a worktree of the base): its commit, and the
                   commits on this checkout's HEAD since that touch the library's inputs, which
                   a "before" run doesn't have

and whether it finished (done, failed, or no end logged: running, or killed, its output folder
perhaps still holding an earlier run's files). What a check reads as it runs (the fixtures, the
saved history) isn't stamped: a fixture changed since shows nowhere here.

    tools/checks.py                      the last day's runs
    tools/checks.py --hours 3
    tools/checks.py --times              how long each check took here: its last finished run
                                         for each scope (the settings that pick what it runs,
                                         SCOPE below), the one before, and the date and commit
    tools/checks.py --times snapshot     and the items (fixtures, states) of that check's last
                                         finished run that grew most against the one before,
                                         for the checks that log them (the replay fixtures,
                                         which `cargo test` runs, and the snapshot)

The times are this machine's (the log is per machine, in its temp folder: a temp clean-up
empties it). How long a check will take is its last run's time, not a figure written down:
the fixtures and the code change it run by run. `cargo test`'s unit tests aren't logged; its
replay fixtures are (`replay_fixtures`), most of its time.
    tools/checks.py /tmp/after ...       every run logged into these folders; exits 1 unless
                                         each has one from this checkout and, for each check that
                                         wrote there, every run since its last full one (without
                                         BAV_ONLY, BAV_HOLDS, BAV_SEEDS or BAV_WHICH: those write part of a
                                         folder) finished on this code
"""
import argparse
import os
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATE = os.path.join(REPO, "crates", "core")
INPUTS = ["src", "../../data/game.json"]  # crates/core/tests/common/source_hash.rs INPUTS
FILTERS = ("BAV_ONLY=", "BAV_HOLDS=", "BAV_SEEDS=", "BAV_WHICH=")
# the settings that pick what a check runs, so its time is compared only with runs of the same
SCOPE = FILTERS + ("BAV_SAMPLE=", "BAV_FINDS=", "BAV_FINDS_QUICK=", "BAV_PLAYON=", "BAV_PLAYON_TRIALS=", "BAV_PLAYON_SIMS=", "BAV_SNAPSHOT_DEADLINE=", "BAV_SEED=")


def temp_dir():
    """Rust's `std::env::temp_dir`: TMP, TEMP on Windows; TMPDIR elsewhere"""
    if os.name == "nt":
        return os.environ.get("TMP") or os.environ.get("TEMP") or os.environ.get("USERPROFILE", ".")
    return os.environ.get("TMPDIR") or "/tmp"


def input_files():
    """(path from the crate, path) of every input file, in source_hash.rs's order"""
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
    """source_hash.rs `source_hash` of this checkout"""
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
    starts, ends, items = [], {}, {}
    try:
        lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
    except FileNotFoundError:
        return starts
    for line in lines:
        f = line.split("\t")
        if len(f) >= 4 and f[1] == "end":
            ends[f[2]] = (f[3], int(f[0]))
        elif len(f) >= 5 and f[1] == "item":
            try:
                items.setdefault(f[2], {})[f[3]] = float(f[4])
            except ValueError:
                pass
        elif len(f) >= 10 and f[1] == "start":
            sources = [x for x in (f[10] if len(f) > 10 else "").split(";") if x]
            starts.append(dict(when=int(f[0]), id=f[2], check=f[3], out=f[4], hash=f[5], crate=f[6], commit=f[7], state=f[8], settings=f[9], sources=sources))
    for s in starts:
        s["end"], s["ended"] = ends.get(s["id"], ("no end logged", None))
        s["items"] = items.get(s["id"], {})
    return starts


def scope(run):
    """The settings that pick what the run ran (SCOPE), files by name: its runs of the same
    scope are the ones its time compares with"""
    out = []
    for st in run["settings"].split():
        if st.startswith(SCOPE):
            k, v = st.split("=", 1)
            out.append(f"{k}={os.path.basename(v) if os.sep in v or '/' in v else v}")
    return " ".join(out) or "(all)"


def took(seconds):
    return f"{seconds / 3600:.1f} h" if seconds >= 5400 else f"{seconds / 60:.0f} min" if seconds >= 90 else f"{seconds:.0f} s"


def times(runs, check):
    """Each check's last finished run here for each scope, against the one before; with `check`,
    that check's items that grew most"""
    done = [r for r in runs if r["end"] == "done" and r["ended"]]
    if check:
        done = [r for r in done if r["check"] == check]
        if not done:
            print(f"no finished run of {check} logged here")
            return 1
    groups = {}
    for r in done:
        groups.setdefault((r["check"], scope(r)), []).append(r)
    print(f"{'check':<24} {'scope':<34} {'last':>8} {'before':>8}  when, on")
    for (name, sc), rs in sorted(groups.items()):
        last, prev = rs[-1], rs[-2] if len(rs) > 1 else None
        n = f", {len(last['items'])} items" if last["items"] else ""
        print(f"{name:<24} {sc[:34]:<34} {took(last['ended'] - last['when']):>8} {took(prev['ended'] - prev['when']) if prev else '-':>8}  "
              f"{time.strftime('%m-%d %H:%M', time.localtime(last['when']))}, {last['commit']}{' with changes' if last['state'] == 'changed' else ''}{n}")
    if check:
        for (name, sc), rs in sorted(groups.items()):
            with_items = [r for r in rs if r["items"]]
            if not with_items:
                continue
            last = with_items[-1]
            prev = with_items[-2] if len(with_items) > 1 else None
            print(f"\n{name} {sc}: its items on {time.strftime('%m-%d %H:%M', time.localtime(last['when']))}"
                  + (f", against {time.strftime('%m-%d %H:%M', time.localtime(prev['when']))} ({prev['commit']})" if prev else ""))
            rows = sorted(last["items"].items(), key=lambda kv: -(kv[1] - (prev["items"].get(kv[0], 0) if prev else 0)))
            for item, secs in rows[:12]:
                before = prev["items"].get(item) if prev else None
                print(f"  {took(secs):>8} {took(before) if before is not None else 'new':>8}  {item}")
            print(f"  total {took(sum(last['items'].values()))}" + (f", before {took(sum(prev['items'].values()))}" if prev else ""))
    return 0


def verdict(run, now_hash, head):
    if same_dir(run["crate"], CRATE):
        # the test's own sources, as its dep-info listed them (from the workspace root)
        tests = [n for n in run["sources"] if os.path.exists(os.path.join(REPO, n)) and os.path.getmtime(os.path.join(REPO, n)) > run["when"]]
        if run["hash"] == now_hash and not tests:
            return "this code", True
        if run["hash"] == now_hash:
            return f"an older test program (changed since: {', '.join(os.path.basename(n.replace(chr(92), '/')) for n in tests)})", False
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
    ap.add_argument("--hours", type=float, default=24, help="how far back to list without folders (default 24)")
    ap.add_argument("--log", default=os.path.join(temp_dir(), "balatro-advisor-checks.tsv"))
    ap.add_argument("--times", nargs="?", const="", metavar="CHECK", help="how long each check took here (with CHECK: its items that grew most)")
    a = ap.parse_args()
    runs = read_log(a.log)
    if a.times is not None:
        return times(runs, a.times) if runs else (print(f"no check runs logged in {a.log}") or 1)
    if not runs:
        print(f"no check runs logged in {a.log}")
        return 1 if a.dirs else 0
    now_hash, head = source_hash(), git("rev-parse", "--short=10", "HEAD")
    print(f"this checkout: HEAD {head}, the library's inputs {now_hash}")
    since = time.time() - a.hours * 3600
    # with folders, every run logged into them, whatever its age: its files may still be there
    shown = [r for r in runs if any(r["out"] and same_dir(r["out"], d) for d in a.dirs)] if a.dirs else [r for r in runs if r["when"] >= since]
    for r in shown:
        what, _ = verdict(r, now_hash, head)
        settings = " ".join(s for s in r["settings"].split() if not (r["out"] and s.split("=", 1)[-1] and same_dir(s.split("=", 1)[-1], r["out"])))
        print(f"{time.strftime('%m-%d %H:%M', time.localtime(r['when']))}  {r['check']:<26} {r['end']:<14} {what}")
        print(f"{'':13} -> {r['out'] or '(no output folder)'}{'  ' + settings if settings else ''}")
    bad = []
    for d in a.dirs:
        here = [r for r in shown if r["out"] and same_dir(r["out"], d) and same_dir(r["crate"], CRATE)]
        # each check writes its own files into the folder: one check's later run says nothing
        # of another's files there (the snapshot's, then the history picks into the same folder)
        mine = []
        for check in dict.fromkeys(r["check"] for r in here):
            its = [r for r in here if r["check"] == check]
            full = [i for i, r in enumerate(its) if not any(s.startswith(FILTERS) for s in r["settings"].split())]
            mine += its[full[-1]:] if full else its
        if not mine:
            bad.append(d)
            print(f"{d}: NO run logged into it from this checkout")
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
