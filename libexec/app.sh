#!/usr/bin/env bash
# ramify app — open the desktop dashboard, installing Electron on first use.
#
# A plugin install is a git snapshot in a versioned cache directory: no node_modules, and a new
# directory on every update. Electron goes to a fixed cache instead, keyed by the version
# app/package.json asks for, so an update does not download it again.
set -uo pipefail

RAMIFY_HOME="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$RAMIFY_HOME/app"
CACHE="${RAMIFY_APP_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/ramify/electron}"

die() { echo "ramify: $*" >&2; exit 1; }

want="$(sed -n 's/.*"electron": *"[^0-9]*\([0-9][0-9.]*\)".*/\1/p' "$APP/package.json")"
[ -n "$want" ] || die "no electron version in $APP/package.json"

# A dev checkout that ran `npm install` in app/ uses its own copy.
pkg="$APP/node_modules/electron"
[ -f "$pkg/path.txt" ] || pkg="$CACHE/node_modules/electron"

if [ "$pkg" = "$CACHE/node_modules/electron" ] \
   && [ "$(sed -n 's/^ *"version": *"\(.*\)".*/\1/p' "$pkg/package.json" 2>/dev/null | head -1)" != "$want" ]; then
  # The Electron installer needs Node ≥ 22.12. The only node on runtime is Electron's own, so
  # this is an install-time need: borrow asdf's newest when the default is older.
  node_ok() { node -e 'const [a, b] = process.versions.node.split(".").map(Number); process.exit(a > 22 || (a === 22 && b >= 12) ? 0 : 1)' 2>/dev/null; }
  if ! node_ok && command -v asdf >/dev/null; then
    ASDF_NODEJS_VERSION="$(asdf list nodejs 2>/dev/null | tr -d ' *' | grep -E '^[0-9]' | sort -V | tail -1)"
    export ASDF_NODEJS_VERSION
  fi
  node_ok && command -v npm >/dev/null \
    || die "installing the app needs Node.js ≥ 22.12 and npm (found $(node -v 2>/dev/null || echo none))"
  echo "installing Electron $want into $CACHE (once, ~100 MB)…"
  mkdir -p "$CACHE"
  # Electron fetches its binary on first use, not at npm install: fetch it now, in the foreground.
  (npm install --prefix "$CACHE" --no-save --no-package-lock --no-audit --no-fund "electron@$want" \
     && node "$CACHE/node_modules/electron/install.js") >"$CACHE/install.log" 2>&1 \
    || die "Electron install failed — see $CACHE/install.log"
fi

bin="$pkg/dist/$(cat "$pkg/path.txt" 2>/dev/null)"
[ -x "$bin" ] || die "Electron is incomplete in $pkg — delete $CACHE and run 'ramify app' again"

# Detached: the window outlives the shell (or the agent turn) that opened it.
nohup "$bin" "$APP" >/dev/null 2>&1 </dev/null &
echo "ramify app started"
