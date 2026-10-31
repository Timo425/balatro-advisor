#!/usr/bin/env bash
# SessionStart hook (.claude/settings.json): bring tests/fixtures/private up to date with the
# private fixtures repo (CLAUDE.md, "Private fixtures"), or say why it can't. Never fails.
d="${CLAUDE_PROJECT_DIR:-.}/tests/fixtures/private"
if [ ! -d "$d" ]; then
  echo "No private fixtures here (a cloud session, or not cloned yet: gh repo clone Timo425/balatro-advisor-fixtures tests/fixtures/private)."
elif [ ! -d "$d/.git" ]; then
  echo "WARNING: tests/fixtures/private is not a clone of the private fixtures repo yet: join it first (CLAUDE.md, \"Private fixtures\")."
elif timeout 60 git -C "$d" pull --ff-only -q 2>/dev/null; then
  dirty=$(git -C "$d" status --porcelain | wc -l)
  ahead=$(git -C "$d" rev-list --count @{u}..HEAD 2>/dev/null || echo 0)
  msg="Fixtures pulled (at $(git -C "$d" log -1 --format='%h, %cr'))."
  [ "$dirty" -gt 0 ] && msg="$msg $dirty uncommitted change(s) in tests/fixtures/private."
  [ "$ahead" -gt 0 ] && msg="$msg $ahead commit(s) not pushed."
  echo "$msg"
else
  echo "WARNING: fixture pull failed (offline, or local changes in the way): tests/fixtures/private may be stale; see git status there."
fi

# The run data (the owner's play history, the bot's runs, calibration, the game file) is made on
# the main PC and backed up to its Proton Drive every 3 hours. Devices share data only through
# Syncthing (2026-10-31): a machine in the main PC's "Proton Drive" Syncthing share copies it from
# that local folder (Windows laptop: balatro-pull.ps1), one outside it still downloads from Proton
# (WSL: ~/.local/bin/balatro-pull); see the vault's advisor-windows-laptop.md /
# advisor-second-machine.md. The pull script runs in the background, at most every 6 hours, and
# every chat is told how fresh the data is. The main PC has neither: skip. A machine's own bot
# runs go the other way, through a send-only Syncthing folder (`bot-<machine>/`, botrun's BAV_BOT_DIR).
data="$HOME/.local/share/balatro-advisor"
stamp="$data/last-pull" result="$data/last-pull.result" log="$data/last-pull.log"
pull=""
if [ -x "$HOME/.local/bin/balatro-pull" ]; then
  pull="nice -n 19 $HOME/.local/bin/balatro-pull"
elif [ -f "$HOME/.local/bin/balatro-pull.ps1" ] && command -v powershell.exe >/dev/null; then
  pull="powershell.exe -NoProfile -ExecutionPolicy Bypass -File $(cygpath -w "$HOME/.local/bin/balatro-pull.ps1" 2>/dev/null || echo "$HOME/.local/bin/balatro-pull.ps1")"
fi
if [ -n "$pull" ]; then
  mkdir -p "$data"
  age_h() { echo $(( ($(date +%s) - $(date -r "$1" +%s)) / 3600 )); }
  if [ ! -f "$stamp" ] || [ "$(age_h "$stamp")" -ge 6 ]; then
    touch "$stamp"
    ( if $pull >"$log" 2>&1; then echo ok >"$result"; else echo "failed (exit $?)" >"$result"; fi ) </dev/null >/dev/null 2>&1 &
    disown 2>/dev/null
    started=" A new pull started in the background."
  fi
  if [ -f "$result" ]; then
    last="$(cat "$result")"
    when="$(age_h "$result")"
    if [ "$last" != ok ]; then
      echo "WARNING: the last pull of the run data (history, bot runs) from the main PC $last, ${when}h ago; see $log.${started:-}"
    elif [ "$when" -ge 48 ]; then
      echo "WARNING: the run data (history, bot runs) was last pulled ${when}h ago: it may be stale.${started:-}"
    else
      echo "Run data (history, bot runs) pulled from the main PC ${when}h ago.${started:-}"
    fi
  else
    echo "Run data (history, bot runs): no finished pull yet on this machine.${started:-}"
  fi
fi
