#!/usr/bin/env bash
# ramify run — hand a script to the human to run: a command the agent may not run itself, such as
# a write to production. The script goes to the run window, which has a Run and a Block button.
#
#   ramify run ask <script> [--cwd DIR] [--note TEXT]   freeze a copy, open the window, print its id
#   ramify run wait <id> [--timeout S]                  block until the human ran or blocked it
#   ramify run go <id>                                  the human's Run: run the frozen copy
#   ramify run block <id> [--note TEXT]                 the human's Block, with an optional reason
#   ramify runs [--json]                                every run, newest first
#   ramify run                                          open the run window
#   ramify run line [root]                              "▶ run <id> waits on you", for card, dash, status line
#
# A run is a folder in the runs folder: `script` (the frozen copy, read-only), `meta.json` and,
# once it ran, `output.log`. Run checks the copy against the hash taken at the ask, so what the
# human read is what runs. `go` and `block` belong to the human: a hook refuses them from the agent.
set -uo pipefail

RAMIFY_HOME="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${RAMIFY_RUNS_DIR:-${CLAUDE_CONFIG_DIR:-$HOME/.claude}/ramify/runs}"
POLL="${RAMIFY_RUN_POLL:-1}"
ID_RE='^[0-9]{8}-[0-9]{6}-[0-9]+$'

die() { echo "ramify: $*" >&2; exit 1; }
usage() { die "usage: ramify run [ask <script> [--cwd DIR] [--note TEXT] | wait <id> | go <id> | block <id> [--note TEXT] | line [root]]"; }

sha() { if command -v shasum >/dev/null; then shasum -a 256 "$1"; else sha256sum "$1"; fi | cut -c1-64; }
now() { date '+%Y-%m-%dT%H:%M:%S'; }

run_dir() { # id → its folder, or die
  [[ "${1:-}" =~ $ID_RE ]] || die "no run id ${1:-} — 'ramify runs' lists them"
  [ -f "$DIR/$1/meta.json" ] || die "no run $1 — 'ramify runs' lists them"
  printf '%s' "$DIR/$1"
}

# meta.json is read and written through python only: a note or a path can hold any character.
meta_get() { # dir, field
  python3 -c 'import json,sys; v=json.load(open(sys.argv[1]+"/meta.json")).get(sys.argv[2]); print("" if v is None else v)' "$1" "$2"
}
meta_set() { # dir, key=value… (values are strings; exit=<n> is stored as a number)
  python3 - "$@" <<'PY'
import json, os, sys
d, pairs = sys.argv[1], sys.argv[2:]
p = os.path.join(d, "meta.json")
m = json.load(open(p))
for kv in pairs:
    k, _, v = kv.partition("=")
    m[k] = int(v) if k == "exit" else v
tmp = p + ".tmp"
with open(tmp, "w") as f:
    json.dump(m, f, indent=2, ensure_ascii=False)
    f.write("\n")
os.replace(tmp, p)
PY
}

ask() {
  local src='' cwd="$PWD" note='' id d root project worktree primary
  while [ $# -gt 0 ]; do
    case "$1" in
      --cwd) [ $# -ge 2 ] || die "--cwd needs a directory"; cwd="$2"; shift 2 ;;
      --note) [ $# -ge 2 ] || die "--note needs a text"; note="$2"; shift 2 ;;
      -*) usage ;;
      *) [ -z "$src" ] || die "one script at a time"; src="$1"; shift ;;
    esac
  done
  [ -n "$src" ] || usage
  [ -f "$src" ] && [ -r "$src" ] || die "no such script: $src"
  [ -s "$src" ] || die "the script is empty: $src"
  [ -d "$cwd" ] || die "no such directory: $cwd"
  cwd="$(cd "$cwd" && pwd -P)"
  src="$(cd "$(dirname "$src")" && pwd -P)/$(basename "$src")"

  mkdir -p "$DIR" && chmod 700 "$DIR"
  id="$(date +%Y%m%d-%H%M%S)-$$"
  d="$DIR/$id"
  mkdir -m 700 "$d" || die "cannot create $d"
  cp "$src" "$d/script" && chmod 500 "$d/script" || die "cannot copy $src"

  root="$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null)"
  primary="$(git -C "$cwd" worktree list --porcelain 2>/dev/null | awk '/^worktree /{print $2; exit}')"
  project="${primary:+$(basename "$primary")}" worktree="${root:+$(basename "$root")}"
  D="$d" ID="$id" CREATED="$(now)" CWD="$cwd" NOTE="$note" SHA="$(sha "$d/script")" SRC="$src" ROOT="$root" \
    PROJECT="$project" WORKTREE="$worktree" python3 -c '
import json, os
e = os.environ
m = {"id": e["ID"], "created": e["CREATED"], "cwd": e["CWD"], "note": e["NOTE"], "sha256": e["SHA"],
     "source": e["SRC"], "root": e["ROOT"] or None, "project": e["PROJECT"] or None,
     "worktree": e["WORKTREE"] or None, "status": "pending"}
with open(os.path.join(e["D"], "meta.json"), "w") as f:
    json.dump(m, f, indent=2, ensure_ascii=False)
    f.write("\n")'
  [ -f "$d/meta.json" ] || die "cannot write $d/meta.json"

  echo "run $id waits on the human — the run window shows $src (frozen at $d/script)"
  echo "  wait for the decision: ramify run wait $id"
  [ "${RAMIFY_POPUP:-1}" = 0 ] && return 0
  RAMIFY_RUNS_DIR="$DIR" "$RAMIFY_HOME/libexec/app.sh" run >/dev/null 2>&1 \
    || echo "  the run window did not open — run 'ramify run', or decide with 'ramify run go|block $id'"
}

# The human's Run. One run at a time per id: the lock is a directory, and mkdir is atomic.
go() {
  local d status want got cwd bash rc
  d="$(run_dir "${1:-}")" || exit 1
  status="$(meta_get "$d" status)"
  [ "$status" = pending ] || die "run ${1} is no longer pending (status: $status)"
  mkdir "$d/lock" 2>/dev/null || die "run ${1} has already started"
  want="$(meta_get "$d" sha256)" got="$(sha "$d/script")"
  if [ "$want" != "$got" ]; then
    meta_set "$d" status=blocked "decided=$(now)" "reason=the frozen script changed after the ask (sha256 $got, asked $want)"
    die "the frozen script changed after the ask — blocked, nothing ran"
  fi
  cwd="$(meta_get "$d" cwd)"
  [ -d "$cwd" ] || { meta_set "$d" status=blocked "decided=$(now)" "reason=the directory $cwd is gone"; die "no such directory: $cwd — blocked, nothing ran"; }
  bash=/opt/homebrew/bin/bash; [ -x "$bash" ] || bash="$(command -v bash)"
  meta_set "$d" status=running "started=$(now)"
  : >"$d/output.log"
  (cd "$cwd" && "$bash" "$d/script" </dev/null 2>&1) | tee -a "$d/output.log"
  rc="${PIPESTATUS[0]}"
  meta_set "$d" status=ran "finished=$(now)" "exit=$rc"
  echo "run ${1} ran: exit $rc"
}

block() {
  local id="${1:-}" note='' d status
  shift || true
  while [ $# -gt 0 ]; do
    case "$1" in
      --note) [ $# -ge 2 ] || die "--note needs a text"; note="$2"; shift 2 ;;
      *) usage ;;
    esac
  done
  d="$(run_dir "$id")" || exit 1
  status="$(meta_get "$d" status)"
  [ "$status" = pending ] || die "run $id is no longer pending (status: $status)"
  mkdir "$d/lock" 2>/dev/null || die "run $id has already started"
  meta_set "$d" status=blocked "decided=$(now)" "reason=$note"
  echo "run $id blocked${note:+: $note}"
}

wait_run() {
  local id="${1:-}" timeout='' d status start reason
  shift || true
  while [ $# -gt 0 ]; do
    case "$1" in
      --timeout) [ $# -ge 2 ] || die "--timeout needs seconds"; timeout="$2"; shift 2 ;;
      *) usage ;;
    esac
  done
  d="$(run_dir "$id")" || exit 1
  start="$(date +%s)"
  while :; do
    status="$(meta_get "$d" status)"
    case "$status" in
      ran)
        echo "ran: exit $(meta_get "$d" exit)"
        [ -s "$d/output.log" ] && tail -n 40 "$d/output.log"
        return 0 ;;
      blocked)
        reason="$(meta_get "$d" reason)"
        echo "blocked: ${reason:-(no reason given)}"
        return 0 ;;
    esac
    if [ -n "$timeout" ] && [ $(( $(date +%s) - start )) -ge "${timeout%.*}" ]; then
      echo "run $id is still $status"
      return 1
    fi
    sleep "$POLL"
  done
}

# Every run as one JSON array, newest first, without the script text (the window reads that).
runs_json() {
  python3 - "$DIR" <<'PY'
import json, os, sys
d = sys.argv[1]
out = []
try:
    names = sorted(os.listdir(d), reverse=True)
except OSError:
    names = []
for n in names:
    try:
        out.append(json.load(open(os.path.join(d, n, "meta.json"))))
    except (OSError, ValueError):
        continue
print(json.dumps(out, ensure_ascii=False))
PY
}

runs() {
  [ "${1:-}" = --json ] && { runs_json; return; }
  runs_json | python3 -c '
import json, sys
runs = json.load(sys.stdin)
if not runs:
    print("no run yet")
for r in runs[:20]:
    state = r["status"] + (" exit %s" % r["exit"] if r.get("exit") is not None else "")
    where = "/".join(x for x in (r.get("project"), r.get("worktree")) if x) or r["cwd"]
    print("  %s  %-12s %s  %s" % (r["id"], state, where, r.get("note") or r["source"]))'
}

# One line per run that waits on the human. With a root, only the runs asked from that worktree.
# Cheap when nothing waits: the status line calls it every few seconds.
line() {
  local hits
  hits="$(grep -l '"status": "pending"' "$DIR"/*/meta.json 2>/dev/null)" || return 0
  printf '%s\n' "$hits" | ROOT="${1:-}" python3 -c '
import json, os, sys
root = os.environ["ROOT"]
for p in sys.stdin.read().split():
    try:
        m = json.load(open(p))
    except (OSError, ValueError):
        continue
    if m.get("status") == "pending" and (not root or m.get("root") == root):
        print("▶ run %s waits on you" % m["id"])'
}

case "${1:-}" in
  '') RAMIFY_RUNS_DIR="$DIR" exec "$RAMIFY_HOME/libexec/app.sh" run ;;
  ask) ask "${@:2}" ;;
  go) go "${2:-}" ;;
  block) block "${@:2}" ;;
  wait) wait_run "${@:2}" ;;
  list|runs) runs "${2:-}" ;;
  line) line "${2:-}" ;;
  *) usage ;;
esac
