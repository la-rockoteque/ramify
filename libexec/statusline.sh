#!/usr/bin/env bash
# ramify statusline [--json] — a Claude Code status line segment for the session's worktree.
#
# Reads the status line JSON on stdin. A session that `ramify new` moved to a worktree is pinned
# to it (the hook writes /tmp/ramify/sessions/<session_id>.root), but its own directory stays
# where it started: without the pin, the status line shows the primary checkout's branch.
#
# Plain: "𖣂 tm-127-pagination ⎇ story/tm-127-pagination", or "𖣂 main" in the primary checkout.
# --json: {"root","worktree","branch","primary","icon","slot","ticket","stack","answering","services"},
# for a status line that draws its own. slot is set while the worktree's stack runs; ticket once
# RAMIFY_JIRA_URL is set. stack is "up", "partial", "down", "stopped", or null without services.
# Prints nothing where ramify is not set up. RAMIFY_STATUSLINE_ICON replaces the tree.
set -uo pipefail

input="$(cat)"
read -r sid cwd < <(printf '%s' "$input" | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
except ValueError:
    d = {}
print(d.get("session_id") or "-", (d.get("workspace") or {}).get("current_dir") or d.get("cwd") or ".")' 2>/dev/null)
pin="${RAMIFY_STATE_DIR:-/tmp/ramify}/sessions/${sid:--}.root"

root=""
# A pin to a tree that is gone, or no longer a checkout (its directory left behind), is stale.
[ -f "$pin" ] && root="$(cat "$pin")" && git -C "$root" rev-parse --git-dir >/dev/null 2>&1 \
  || root="$(git -C "${cwd:-.}" rev-parse --show-toplevel 2>/dev/null)"
[ -n "$root" ] || exit 0
primary="$(git -C "$root" worktree list --porcelain 2>/dev/null | awk '/^worktree /{print $2; exit}')"

# Set up = a config in this tree or in the primary checkout, as `ramify` itself looks for one.
conf=""
for d in "$root" "$primary"; do
  for f in .ramify.conf .claude/ramify.conf; do [ -z "$conf" ] && [ -n "$d" ] && [ -f "$d/$f" ] && conf="$d/$f"; done
done
[ -n "$conf" ] || exit 0

branch="$(git -C "$root" rev-parse --abbrev-ref HEAD 2>/dev/null)" || branch='?'
icon="${RAMIFY_STATUSLINE_ICON:-𖣂}"
wt="$(basename "$root")"

# ponytail: read, not sourced — the status line runs every few seconds. A config that sets
# RAMIFY_PROJECT or RAMIFY_TICKET_PATTERN by computation is misread; source it if that happens.
project="$(sed -n "s/^RAMIFY_PROJECT=['\"]*\([^'\"]*\).*/\1/p" "$conf" | tail -1)"
env="${RAMIFY_STATE_DIR:-/tmp/ramify}/${project:-$(basename "$primary")}/$wt.env"
slot="$(sed -n 's/^SLOT=//p' "$env" 2>/dev/null | tail -1)"

# Any HTTP answer counts, even an error page: the question is whether the process serves, not
# whether its health path is green. One request per port, all at once, a second at most.
stack=""; answering=0; total=0
if grep -q '^RAMIFY_SERVICES=.' "$conf"; then
  stack=stopped
  if [ -f "$env" ]; then
    ports="$(sed -n 's/^[A-Z0-9_]*_PORT=\([0-9]*\)$/\1/p' "$env")"
    total="$(printf '%s' "$ports" | grep -c .)"
    answering="$(for p in $ports; do (curl -s -o /dev/null --max-time 1 "http://localhost:$p/" && echo ok) & done; wait)"
    answering="$(printf '%s' "$answering" | grep -c ok)"
    if [ "$total" = 0 ] || [ "$answering" = 0 ]; then stack=down
    elif [ "$answering" = "$total" ]; then stack=up
    else stack=partial; fi
  fi
fi
case "$stack" in
  up|partial) health=" · $answering/$total up" ;;
  down) health=" · stack down" ;;
  *) health="" ;;
esac

if [ "${1:-}" = --json ]; then
  ticket=""
  if grep -q '^RAMIFY_JIRA_URL=.' "$conf"; then
    pattern="$(sed -n "s/^RAMIFY_TICKET_PATTERN=['\"]*\([^'\"]*\).*/\1/p" "$conf" | tail -1)"
    s="-${branch//\//-}-"
    [[ "$s" =~ -(${pattern:-[A-Za-z][A-Za-z0-9]*-[0-9]+})- ]] && ticket="$(printf '%s' "${BASH_REMATCH[1]}" | tr '[:lower:]' '[:upper:]')"
  fi
  STACK="$stack" ANSWERING="$answering" TOTAL="$total" SLOT="$slot" TICKET="$ticket" ROOT="$root" WT="$wt" BRANCH="$branch" PRIMARY="$([ "$root" = "$primary" ] && echo 1)" ICON="$icon" python3 -c '
import json, os
e = os.environ
print(json.dumps({"root": e["ROOT"], "worktree": e["WT"], "branch": e["BRANCH"], "primary": e["PRIMARY"] == "1", "icon": e["ICON"],
                  "slot": int(e["SLOT"]) if e["SLOT"].isdigit() else None, "ticket": e["TICKET"] or None,
                  "stack": e["STACK"] or None, "answering": int(e["ANSWERING"]), "services": int(e["TOTAL"])}))'
elif [ "$root" = "$primary" ]; then
  printf '%s %s%s\n' "$icon" "$branch" "$health"
else
  printf '%s %s ⎇ %s%s\n' "$icon" "$wt" "$branch" "$health"
fi
