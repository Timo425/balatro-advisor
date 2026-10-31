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
