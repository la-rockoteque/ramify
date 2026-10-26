#!/usr/bin/env bash
# ramify scratch — hand a piece of text to the human to copy: a message to paste, a command, a
# ticket body. The text goes to the scratch window, which has a Copy button.
#
#   ramify scratch [--title T] [FILE | -]   add an item (stdin without FILE) and open the window
#   ramify scratch --list                   the items that wait in the window
#   ramify scratch --clear                  drop every item
#
# An item is two files in the scratch folder: <id>.txt holds the text, <id>.title its title.
# The window reads the folder, and its Done button deletes the pair.
set -uo pipefail

RAMIFY_HOME="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${RAMIFY_SCRATCH_DIR:-${RAMIFY_STATE_DIR:-/tmp/ramify}/scratch}"

die() { echo "ramify: $*" >&2; exit 1; }

title='' src=''
while [ $# -gt 0 ]; do
  case "$1" in
    --title) [ $# -ge 2 ] || die "--title needs a value"; title="$2"; shift 2 ;;
    --list)
      found=0
      for f in "$DIR"/*.txt; do
        [ -f "$f" ] || continue; found=1
        printf '  %s  %s\n' "$(basename "$f" .txt)" "$(cat "${f%.txt}.title" 2>/dev/null)"
      done
      [ "$found" = 1 ] || echo "nothing in the scratch window"
      exit 0 ;;
    --clear) rm -f "$DIR"/*.txt "$DIR"/*.title; echo "scratch cleared"; exit 0 ;;
    -) src=-; shift ;;
    -*) die "unknown option $1 — usage: ramify scratch [--title T] [FILE | -]" ;;
    *) [ -z "$src" ] || die "one file at a time"; src="$1"; shift ;;
  esac
done

mkdir -p "$DIR"
id="$(date +%Y%m%d-%H%M%S)-$$"
if [ -z "$src" ] || [ "$src" = - ]; then
  [ -t 0 ] && die "no text: pass a FILE, or pipe the text in"
  cat >"$DIR/$id.txt"
else
  [ -f "$src" ] || die "no such file: $src"
  cat "$src" >"$DIR/$id.txt"
  [ -n "$title" ] || title="$(basename "$src")"
fi
[ -s "$DIR/$id.txt" ] || { rm -f "$DIR/$id.txt"; die "the text is empty"; }
printf '%s' "${title:-$(head -1 "$DIR/$id.txt" | cut -c1-60)}" >"$DIR/$id.title"
echo "scratch item $id added"

[ "${RAMIFY_POPUP:-1}" = 0 ] && exit 0
RAMIFY_SCRATCH_DIR="$DIR" "$RAMIFY_HOME/libexec/app.sh" scratch \
  || echo "  the scratch window did not open — run 'ramify scratch --list', or read $DIR/$id.txt"
