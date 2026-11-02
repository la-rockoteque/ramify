#!/usr/bin/env bash
# Check a branch's diff against the write set its QA plan declared.
#
# Usage:
#   ramify write-set path/to/write-set.txt
#   ramify write-set --self-test                   # the checker checks itself
#   BASE_REF=origin/develop ramify write-set <file>   # another base branch
#
# A write set names the paths a branch may touch and the ones it must not. Written as prose in a
# plan, a reviewer has to hold it in their head; this turns it into a check.
#
# The write-set file is a list of glob patterns, one per line:
#   path/glob        the branch MAY change this path        (allowed)
#   !path/glob       the branch MUST NOT change this path   (immutable)
#   # comment        ignored, as is a blank line
#
# `*` matches any run of characters, the `/` separator included, so `docs/adr/*` covers a whole
# subtree. An immutable pattern WINS over an allowed one. Do not write an immutable pattern that
# covers a path the branch must edit. Name the siblings instead. A path that matches no allowed
# pattern is outside the write set. This guards every directory the file never mentions.
#
# The compared set is the three-dot diff `BASE_REF...HEAD` — what THIS branch changed, never what
# the base moved on to — PLUS the working tree, staged or not, untracked files included. The
# working tree is in scope on purpose. You cannot run a committed-history-only checker before the
# commit that breaks the boundary.
#
# A MOVE is two paths in this diff: the deletion and the addition. Name both, or the checker
# refuses the branch on the path the move left behind.
#
# Exit codes: 0 clean · 1 the diff breaks the write set (the offending paths are printed) ·
# 2 the checker could not run (no file, no such base ref, an empty write set).

set -euo pipefail

# ── the checker's own negative test ──────────────────────────────────────────────────────────
# A checker that cannot fail proves nothing. Both refusals are exercised against a FIXED path list rather than against whatever the branch
# happens to hold, so the result does not depend on the day it is run.
#
# WHAT IT DOES NOT COVER, so that "self-test passed" is never read as "the checker works". It injects CHECK_WRITE_SET_PATHS, so it exercises the MATCHING half
# only: allowed patterns, immutable patterns, and the precedence between them. The half that
# decides WHICH PATHS ARE JUDGED is not exercised — the three-dot diff against the base ref, its
# union with the working tree, and what happens when git itself fails. test/smoke.sh runs it on a
# real branch.
if [[ "${1:-}" == "--self-test" ]]; then
  self_dir="$(mktemp -d)"
  trap 'rm -rf "$self_dir"' EXIT
  paths=$'src/api/orders.ts\ndocs/adr/0001-pipeline.md'

  printf '%s\n' 'src/*' 'docs/adr/*' > "$self_dir/pass.txt"
  printf '%s\n' 'src/*' > "$self_dir/omits.txt"
  printf '%s\n' 'src/*' 'docs/adr/*' '!docs/adr/*' > "$self_dir/immutable.txt"

  status=0
  run_case() { # name, fixture, expected exit, path that must be named
    local name="$1" fixture="$2" expected="$3" needle="${4:-}" out rc
    out="$(CHECK_WRITE_SET_PATHS="$paths" "$0" "$fixture" 2>&1)" && rc=0 || rc=$?
    if [[ "$rc" != "$expected" ]]; then
      echo "self-test FAIL [$name]: expected exit $expected, got $rc"; echo "$out"; status=1; return
    fi
    if [[ -n "$needle" && "$out" != *"$needle"* ]]; then
      echo "self-test FAIL [$name]: the output never names $needle"; echo "$out"; status=1; return
    fi
    echo "self-test ok   [$name]"
  }

  # The control: the same two paths pass when the write set allows them. Without it the two
  # refusals below could both be passing for the wrong reason.
  run_case "a covered diff passes" "$self_dir/pass.txt" 0
  # A path the write set never names is OUTSIDE it.
  run_case "an omitted path fails" "$self_dir/omits.txt" 1 "docs/adr/0001-pipeline.md"
  # A path an immutable pattern names fails even though an allowed pattern also names it.
  run_case "an immutable path fails" "$self_dir/immutable.txt" 1 "docs/adr/0001-pipeline.md"

  [[ $status -eq 0 ]] && echo "check-write-set: self-test passed (pattern matching only — the diff half is not covered here)."
  exit $status
fi

WRITE_SET="${1:-}"
BASE_REF="${BASE_REF:-origin/main}"

if [[ -z "$WRITE_SET" ]]; then
  echo "usage: ramify write-set <write-set-file> | --self-test" >&2
  exit 2
fi

if [[ ! -f "$WRITE_SET" ]]; then
  echo "check-write-set: no write set at $WRITE_SET" >&2
  exit 2
fi

allowed=()
immutable=()

while IFS= read -r line || [[ -n "$line" ]]; do
  line="${line%$'\r'}"                              # tolerate a CRLF checkout
  line="${line#"${line%%[![:space:]]*}"}"           # trim leading blanks
  line="${line%"${line##*[![:space:]]}"}"           # trim trailing blanks
  [[ -z "$line" || "$line" == '#'* ]] && continue
  if [[ "$line" == '!'* ]]; then
    immutable+=("${line#!}")
  else
    allowed+=("$line")
  fi
done < "$WRITE_SET"

if [[ ${#allowed[@]} -eq 0 ]]; then
  echo "check-write-set: $WRITE_SET declares no allowed pattern — every change would be a violation." >&2
  exit 2
fi

# CHECK_WRITE_SET_PATHS is the seam the self-test above drives the checker through. It is read
# only, and only when it is set; a normal run always asks git.
if [[ -n "${CHECK_WRITE_SET_PATHS-}" ]]; then
  changed="$CHECK_WRITE_SET_PATHS"
else
  if ! git rev-parse --verify --quiet "$BASE_REF" >/dev/null; then
    echo "check-write-set: no such base ref: $BASE_REF" >&2
    exit 2
  fi
  # A rename reads "old -> new" on one porcelain line. Both halves are paths this branch
  # touched — a file moved OUT of the write set is a violation as much as one moved in — so the
  # line is split before the quotes a path with a space carries are stripped.
  changed=$(
    {
      git diff --name-only "$BASE_REF...HEAD"
      git status --porcelain --untracked-files=all | cut -c4- | sed 's/ -> /\n/'
    } | sed 's/^"\(.*\)"$/\1/' | sed '/^[[:space:]]*$/d' | sort -u
  )
fi

# Any pattern in the list matches this path?
matches_any() {
  local path="$1"
  shift
  local pattern
  for pattern in "$@"; do
    # Unquoted on purpose: this is where the glob is applied.
    # shellcheck disable=SC2254
    case "$path" in
      $pattern) return 0 ;;
    esac
  done
  return 1
}

violations=0

while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  if matches_any "$path" ${immutable[@]+"${immutable[@]}"}; then
    echo "IMMUTABLE  $path"
    violations=$((violations + 1))
  elif ! matches_any "$path" "${allowed[@]}"; then
    echo "OUTSIDE    $path"
    violations=$((violations + 1))
  fi
done <<< "$changed"

if [[ $violations -gt 0 ]]; then
  echo
  echo "check-write-set: $violations path(s) break the write set in $WRITE_SET."
  echo "A path that genuinely belongs to this branch is ADDED to the write set as a deviation,"
  echo "with the reason beside it. It is never waved through."
  exit 1
fi

echo "check-write-set: the diff stays inside $WRITE_SET."
