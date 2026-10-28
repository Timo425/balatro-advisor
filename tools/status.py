#!/usr/bin/env python3
"""Where balatro-advisor stands, printed on demand from the repo and the local data, never stored.
Run it, and read docs/owner-view.md, before brainstorming, judging a gap or picking work
(CLAUDE.md).

    tools/status.py
    tools/status.py --stale-days 7 --stale-commits 20

Four parts:
- code: HEAD and the recent commits, counted by their stage prefix.
- the owner's play, from ~/.local/share/balatro-advisor/history/<seed>/ (the live page's saved
  states). The game deletes its save on a game over, so no state shows the loss: a run is
  "won" when its last state has `won` set, "lost" (most likely) when its last state is a blind's
  last hand with the target not reached and nothing after it, and "left" when the saves stop
  between hands or with hands left (a restart, a quit, or the run still going). Ante 1 runs that
  ended are counted apart: the owner restarts weak starts there. The runs span many advisor
  versions.
- the bot, from ~/.local/share/balatro-advisor/bot/runs.jsonl, `void` runs left out (see
  tools/botrun.py's docstring), by the commit each run played.
- every numbered gap in docs/design.md "Known gaps" in one line: the replay fixtures waiting on
  it (`pending_gap`), the commits that name it (`git log --grep`: noisy, it matches the body and
  passing mentions) and the owner's impact rating from docs/owner-view.md.

What isn't on this machine (a cloud session has only tracked files) prints "not available
here". A measurement older than --stale-days or --stale-commits is marked STALE.
"""
import argparse
import collections
import datetime
import json
import os
import pathlib
import re
import shutil
import subprocess

REPO = pathlib.Path(__file__).resolve().parent.parent
# the advisor's data folder, as calibration.rs `default_dir` finds it
DATA = pathlib.Path(os.environ.get("XDG_DATA_HOME") or pathlib.Path.home() / ".local/share") / "balatro-advisor"
STAGES = ("valuation", "simulated player", "moves tried", "noise", "engine", "tools", "docs", "refactor")


def git(*args):
    out = subprocess.run(["git", "-C", str(REPO), *args], capture_output=True, text=True)
    return out.stdout.strip() if out.returncode == 0 else ""


def day(ts):
    """A Unix time as a local date (the owner's time zone)."""
    return datetime.datetime.fromtimestamp(ts).strftime("%Y-%m-%d")


def age_days(ts):
    return (datetime.datetime.now().timestamp() - ts) / 86400


def stale(days=None, commits=None, args=None):
    why = []
    if days is not None and days > args.stale_days:
        why.append(f"{days:.0f} days")
    if commits is not None and commits > args.stale_commits:
        why.append(f"{commits} commits")
    return f"  STALE ({', '.join(why)} old)" if why else ""


def commits_since(ts):
    """Commits on HEAD made after a Unix time."""
    out = git("rev-list", "--count", f"--since={int(ts)}", "HEAD")
    return int(out) if out else None


def code(args):
    print("## Code")
    head = git("log", "-1", "--format=%h %as %s")
    if not head:
        print("not available here (not a git checkout)")
        return
    print(f"HEAD {head}")
    since = (datetime.date.today() - datetime.timedelta(days=args.days)).isoformat()
    subjects = git("log", f"--since={since}", "--format=%s").splitlines()
    by = collections.Counter()
    for s in subjects:
        prefix = s.split(":", 1)[0].lower() if ":" in s else ""
        for p in [x.strip() for x in prefix.split(",")] or [""]:
            by[p if p in STAGES else "other"] += 1
    print(f"commits in the last {args.days} days: {len(subjects)} ({', '.join(f'{k} {v}' for k, v in by.most_common())})"
          "  [a commit with two stage prefixes counts in both]")
    for line in git("log", "-8", "--format=%h %as %s").splitlines():
        print("  " + line[:150])


def run_state(path):
    """The advisor's reading of one saved state, or None (no advisor installed, or it can't read it)."""
    if not shutil.which("balatro-advisor"):
        return None
    out = subprocess.run(["balatro-advisor", "state", "--file", str(path), "--json"], capture_output=True, text=True)
    try:
        return json.loads(out.stdout)
    except ValueError:
        return None


def ending(s):
    """How a run's saves end: won, lost (most likely) or left, from its last state (module docstring)."""
    if s.get("won"):
        return "won"
    blind = s.get("current_blind") or {}
    if s.get("screen") == "selecting_hand" and s.get("hands_left") == 1 and blind.get("scored", 0) < blind.get("target", 0):
        return "lost"
    return "left"


def owner(args):
    print("\n## The owner's play (history)")
    hist = DATA / "history"
    runs = sorted(p for p in hist.glob("*") if p.is_dir() and any(p.glob("*.jkr"))) if hist.is_dir() else []
    if not runs:
        print(f"not available here (no {hist})")
        return
    rows = []
    for d in runs:
        files = sorted(d.glob("*.jkr"))
        first, last = files[0].name, files[-1].name
        s = run_state(files[-1]) or {}
        rows.append({
            "seed": d.name, "states": len(files),
            "start": int(first.split("-")[0]) / 1000, "end": int(last.split("-")[0]) / 1000,
            "first_ante": int(re.search(r"-a(\d+)-", first).group(1)),
            "ante": s.get("ante", int(re.search(r"-a(\d+)-", last).group(1))),
            "deck": s.get("deck", "?"), "stake": s.get("stake", "?"),
            "ended": ending(s) if s else "?",
        })
    rows.sort(key=lambda r: r["start"])
    newest = max(r["end"] for r in rows)
    print(f"{len(rows)} runs, {day(rows[0]['start'])} .. {day(newest)}; newest saved state {day(newest)}{stale(days=age_days(newest), commits=commits_since(newest), args=args)}"
          f" (a copy pulled from another machine is as new as its last pull; the history keeps the newest runs, up to 500 MB / 90 days)")
    print(f"advisor versions: the runs span {commits_since(rows[0]['start']) or '?'} commits since the first, "
          f"{commits_since(newest) or 0} of them after the newest state: they judge the advisor as it was then")
    if not shutil.which("balatro-advisor"):
        print("balatro-advisor isn't installed: deck, stake and endings unknown ('?'), antes from file names")
    by = collections.Counter((r["deck"], r["stake"]) for r in rows)
    print("deck and stake: " + ", ".join(f"{d} stake {s} x{n}" for (d, s), n in by.most_common()))
    ends = collections.Counter(r["ended"] for r in rows)
    print(f"endings: won {ends['won']}, lost (last hand short, nothing after) {ends['lost']}, "
          f"left (saves stop between hands or with hands left) {ends['left']}" + (f", unknown {ends['?']}" if ends["?"] else ""))
    a1 = [r for r in rows if r["ante"] == 1 and r["ended"] != "won"]
    print(f"ended in Ante 1 (the owner's restarts, or a loss there; the saves can't tell): {len(a1)}"
          f" ({sum(r['ended'] == 'left' for r in a1)} left, {sum(r['ended'] == 'lost' for r in a1)} on a last hand)")
    reached = collections.Counter(r["ante"] for r in rows if r["ended"] != "won")
    print("ante reached, runs not won: " + ", ".join(f"a{a} x{n}" for a, n in sorted(reached.items())))
    late = [r["seed"] for r in rows if r["first_ante"] > 1]
    if late:
        print(f"saved from mid-run only (the live page started late): {', '.join(late)}")
    print("last 8 runs: " + "; ".join(f"{day(r['start'])} {r['seed']} a{r['ante']} {r['ended']}" for r in rows[-8:]))


def bot(args):
    print("\n## The bot (tools/botrun.py)")
    path = DATA / "bot/runs.jsonl"
    if not path.is_file():
        print(f"not available here (no {path})")
        return
    runs = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    valid = [r for r in runs if not r.get("void")]
    print(f"{len(runs)} runs logged, {len(runs) - len(valid)} void (not evidence: tools/botrun.py's docstring)")
    if not valid:
        print("no valid bot runs: nothing measured about the advisor here")
        return
    groups = collections.defaultdict(list)
    for r in valid:
        groups[r.get("commit") or "?"].append(r)
    for c, rs in sorted(groups.items(), key=lambda kv: max(r["time"] for r in kv[1])):
        behind = git("rev-list", "--count", f"{c}..HEAD") if c != "?" else ""
        behind = int(behind) if behind else None
        last = max(r["time"] for r in rs)
        decks = collections.Counter(f"{r['deck']}/{r['stake']}" for r in rs)
        antes = sorted(r["ante"] for r in rs)
        reach = sorted(r["ante2_reach"] for r in rs if r.get("ante2_reach") is not None)
        missed = collections.Counter(m.split(":", 1)[0] + ":" + " ".join(m.split(":", 1)[1].split()[:2]) if ":" in m else m
                                     for r in rs for m in r.get("not_followed") or [])
        print(f"{c} ({behind if behind is not None else '?'} commits behind HEAD, last run {day(last)}){stale(days=age_days(last), commits=behind, args=args)}: "
              f"{len(rs)} runs ({', '.join(f'{k} x{n}' for k, n in decks.items())}), antes {antes}, "
              f"wins {sum(1 for r in rs if r.get('won'))}, restarts {sum(1 for r in rs if r.get('restarted'))}"
              + (f", ante2_reach median {reach[len(reach) // 2]:.2f}" if reach else ""))
        if missed:
            print("  most common not followed: " + ", ".join(f"{k} x{n}" for k, n in missed.most_common(3)))


def ratings():
    """The owner's impact rating for each gap, from docs/owner-view.md's table."""
    p = REPO / "docs/owner-view.md"
    out = {}
    if p.is_file():
        for m in re.finditer(r"^\|\s*(\d+)\s*\|[^|]*\|\s*([^|]*?)\s*\|", p.read_text(), re.M):
            out[int(m.group(1))] = m.group(2) or "?"
    return out


def gaps(args):
    print("\n## Known gaps (docs/design.md)")
    text = (REPO / "docs/design.md").read_text()
    known = text[text.find("\n## Known gaps"):]
    found = [(n, " ".join(t.split())) for n, t in re.findall(r"^(\d+)\. \*\*(.+?)\*\*", known, re.M | re.S)]  # titles wrap
    pending = collections.defaultdict(list)
    fixtures = REPO / "tests/fixtures/private"
    have_fixtures = fixtures.is_dir()
    for f in sorted(fixtures.rglob("*.json")) if have_fixtures else []:
        try:
            g = json.loads(f.read_text()).get("pending_gap", "")
        except (ValueError, AttributeError):
            continue
        m = re.match(r"gap (\d+)", g or "")
        if m:
            pending[int(m.group(1))].append(f.stem)
    rate = ratings()
    print("columns: gap. title | fixtures waiting on it (pending_gap) | commits naming it, last date "
          "(git log --grep: noisy) | the owner's rating")
    if not have_fixtures:
        print("(fixtures: not available here)")
    for n, title in found:
        n = int(n)
        log = git("log", "-E", "-i", f"--grep=gap {n}([^0-9]|$)", "--format=%as").splitlines()
        fx = ", ".join(pending[n]) if pending[n] else "-"
        print(f"{n:>2}. {title[:90]} | {fx if have_fixtures else 'n/a'} | {len(log)}, {log[0] if log else '-'} | {rate.get(n, '?')}")


def view(args):
    p = REPO / "docs/owner-view.md"
    print("\n## The owner's view (docs/owner-view.md)")
    if not p.is_file():
        print("missing")
        return
    dated = re.findall(r"^- \*\*(\d{4}-\d{2}-\d{2})\*\*", p.read_text(), re.M)
    if dated:
        newest = max(dated)
        ts = datetime.datetime.fromisoformat(newest).timestamp()
        print(f"{len(dated)} dated lines, the newest {newest}{stale(days=age_days(ts), commits=commits_since(ts), args=args)}; "
              f"{sum(1 for v in ratings().values() if v != '?')} gaps rated. Read it whole.")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--days", type=int, default=14, help="the window of recent commits counted by stage")
    p.add_argument("--stale-days", type=float, default=7, help="a measurement older than this is marked STALE")
    p.add_argument("--stale-commits", type=int, default=20, help="or one made this many commits before HEAD")
    args = p.parse_args()
    code(args)
    owner(args)
    bot(args)
    gaps(args)
    view(args)


if __name__ == "__main__":
    main()
