#!/usr/bin/env python3
"""Screenshots of the live page on a saved state: what the owner sees, for checking page text and
layout before a commit (CLAUDE.md: "The live page's text is for the owner mid-game").

Runs `balatro-advisor ui` on the state (its own scratch data folder, so the history and timings
stay untouched), keeps each screen the page is served on the way (the first ranking, the full
analysis), then draws each in headless Chrome from a small local server that serves the page with
that one body, so a screen that lasts seconds live holds still for the picture.

Usage: tools/page_shot.py <state.jkr> [--out DIR] [--binary PATH] [--size WxH]
Chrome: $BAV_CHROME, else chrome-for-testing, google-chrome, chromium or chromium-browser on PATH.
Prints the PNG paths (first.png when a first ranking was made, full.png)."""
import argparse, http.server, json, os, shutil, subprocess, sys, tempfile, threading, time, urllib.request

ap = argparse.ArgumentParser()
ap.add_argument("state")
ap.add_argument("--out", default=".")
ap.add_argument("--binary", default="balatro-advisor")
ap.add_argument("--size", default="1250,1700")
args = ap.parse_args()

chrome = os.environ.get("BAV_CHROME") or next((c for c in ("chrome-for-testing", "google-chrome", "chromium", "chromium-browser") if shutil.which(c)), None)
if not chrome:
    sys.exit("no Chrome found: set BAV_CHROME")
os.makedirs(args.out, exist_ok=True)
tmp = tempfile.mkdtemp(prefix="page-shot-")
PORT, SHOT_PORT = 7793, 7794


def get(path):
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}{path}", timeout=2) as r:
        return r.read()


# 1. The real server on the saved state: the page, and each body it serves
os.makedirs(os.path.join(tmp, "save", "1"))
shutil.copy(args.state, os.path.join(tmp, "save", "1", "save.jkr"))
env = dict(os.environ, XDG_DATA_HOME=os.path.join(tmp, "xdg"))
server = subprocess.Popen(["nice", "-n", "19", args.binary, "ui", "--save-dir", os.path.join(tmp, "save"), "--profile", "1", "--port", str(PORT), "--no-open"],
                          env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
page, bodies, t0 = None, {}, time.time()
try:
    while time.time() - t0 < 300:
        try:
            page = page or get("/")
            raw = get("/api/analysis")
        except Exception:
            time.sleep(0.25)
            continue
        w = json.loads(raw)
        b = w.get("data") or {}
        if b.get("first") and "first" not in bodies:
            bodies["first"] = raw
            print(f"first ranking at {time.time() - t0:.1f}s", file=sys.stderr)
        if b.get("status") == "ok" and not b.get("partial") and not w.get("busy"):
            bodies["full"] = raw
            print(f"full analysis at {time.time() - t0:.1f}s", file=sys.stderr)
            break
        time.sleep(0.2)
finally:
    server.terminate()
    server.wait()
if "full" not in bodies:
    sys.exit("the full analysis didn't come within 300 s")

# 2. Each body, held still, drawn by Chrome
current = {}


class Still(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body, kind = (current["raw"], "application/json") if self.path.startswith("/api/analysis") else (page, "text/html; charset=utf-8")
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        self.send_response(200)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *a):
        pass


still = http.server.ThreadingHTTPServer(("127.0.0.1", SHOT_PORT), Still)
threading.Thread(target=still.serve_forever, daemon=True).start()
for name in ("first", "full"):
    if name not in bodies:
        continue
    current["raw"] = bodies[name]
    png = os.path.abspath(os.path.join(args.out, f"{name}.png"))
    subprocess.run([chrome, "--headless=new", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", f"--user-data-dir={os.path.join(tmp, 'chrome')}",
                    f"--window-size={args.size}", "--virtual-time-budget=4000", f"--screenshot={png}", f"http://127.0.0.1:{SHOT_PORT}/"],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
    print(png if os.path.exists(png) else f"no picture for {name}")
still.shutdown()
shutil.rmtree(tmp, ignore_errors=True)
