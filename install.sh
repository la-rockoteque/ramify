#!/usr/bin/env bash
# install.sh — install ram (Ram Code) and the ramify CLI it drives.
#
# From a checkout:   ./install.sh
# From the remote:   curl -fsSL https://raw.githubusercontent.com/la-rockoteque/ramify/main/install.sh | bash
#
# It builds both release binaries and links bin/ram and bin/ramify into a directory on PATH.
# Run it again to update. `--uninstall` removes the links.
set -uo pipefail

REPO="${RAMIFY_REPO:-https://github.com/la-rockoteque/ramify.git}"
REF="${RAMIFY_REF:-main}"
BIN_DIR="${RAMIFY_BIN_DIR:-$HOME/.local/bin}"
HOME_DIR="${RAMIFY_HOME:-}"
BUILD=1 FORCE=0 UNINSTALL=0
NAMES="ram ramify"

usage() {
  cat <<EOF
Usage: install.sh [--bin-dir DIR] [--home DIR] [--no-build] [--force] [--uninstall]

  --bin-dir DIR  Where to link ram and ramify (default: ~/.local/bin, or \$RAMIFY_BIN_DIR)
  --home DIR     The ramify checkout to install from. Default: this script's checkout, else a
                 clone at ~/.local/share/ramify (or \$RAMIFY_HOME)
  --no-build     Link only. The launchers build the binaries on first use
  --force        Replace a file at the link path that is not a symlink
  --uninstall    Remove the links that point into the checkout
EOF
}

say()  { printf '%s\n' "$*"; }
die()  { printf 'install: %s\n' "$*" >&2; exit 1; }
warn() { printf 'install: warning: %s\n' "$*" >&2; }

while [ $# -gt 0 ]; do
  case "$1" in
    --bin-dir) [ $# -ge 2 ] || die "--bin-dir needs a directory"; BIN_DIR="$2"; shift 2 ;;
    --bin-dir=*) BIN_DIR="${1#*=}"; shift ;;
    --home) [ $# -ge 2 ] || die "--home needs a directory"; HOME_DIR="$2"; shift 2 ;;
    --home=*) HOME_DIR="${1#*=}"; shift ;;
    --no-build) BUILD=0; shift ;;
    --force) FORCE=1; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
done

is_checkout() { [ -f "$1/Cargo.toml" ] && [ -x "$1/bin/ram" ] && [ -x "$1/bin/ramify" ]; }

# Piped through bash, BASH_SOURCE is empty or names no file: there is no checkout beside it.
here=""
script="${BASH_SOURCE[0]:-}"
if [ -n "$script" ] && [ -f "$script" ]; then
  here="$(cd "$(dirname "$script")" && pwd -P)"
fi

clone=0
if [ -z "$HOME_DIR" ]; then
  if [ -n "$here" ] && is_checkout "$here"; then HOME_DIR="$here"
  else HOME_DIR="$HOME/.local/share/ramify"; clone=1
  fi
fi

if [ "$UNINSTALL" = 1 ]; then
  [ -d "$HOME_DIR" ] && HOME_DIR="$(cd "$HOME_DIR" && pwd -P)"
  for name in $NAMES; do
    link="$BIN_DIR/$name"
    if [ -L "$link" ]; then
      dest="$(readlink "$link")"
      case "$dest" in
        "$HOME_DIR"/bin/*) rm -f "$link" && say "removed $link" ;;
        *) warn "$link points to $dest, not into $HOME_DIR; left in place" ;;
      esac
    elif [ -e "$link" ]; then
      warn "$link is not a symlink; left in place"
    fi
  done
  [ "$clone" = 1 ] && [ -d "$HOME_DIR" ] && say "the clone at $HOME_DIR stays; remove it with: rm -rf \"$HOME_DIR\""
  exit 0
fi

command -v git >/dev/null 2>&1 || die "git is required"

if [ "$clone" = 1 ]; then
  if [ -d "$HOME_DIR/.git" ]; then
    say "updating $HOME_DIR"
    git -C "$HOME_DIR" fetch --quiet origin "$REF" || die "git fetch failed in $HOME_DIR"
    git -C "$HOME_DIR" checkout --quiet "$REF" || die "git checkout $REF failed in $HOME_DIR"
    git -C "$HOME_DIR" merge --quiet --ff-only "origin/$REF" \
      || die "$HOME_DIR has local changes that block a fast-forward to origin/$REF"
  elif [ -e "$HOME_DIR" ]; then
    die "$HOME_DIR exists and is not a git checkout; pass --home or remove it"
  else
    say "cloning $REPO into $HOME_DIR"
    mkdir -p "$(dirname "$HOME_DIR")" || die "cannot create $(dirname "$HOME_DIR")"
    git clone --quiet --branch "$REF" "$REPO" "$HOME_DIR" || die "git clone failed"
  fi
fi

[ -d "$HOME_DIR" ] || die "$HOME_DIR does not exist"
HOME_DIR="$(cd "$HOME_DIR" && pwd -P)"
is_checkout "$HOME_DIR" || die "$HOME_DIR is not a ramify checkout (no Cargo.toml, bin/ram or bin/ramify)"

if [ "$BUILD" = 1 ]; then
  command -v cargo >/dev/null 2>&1 \
    || die "cargo is required to build; install Rust from https://rustup.rs, or pass --no-build"
  say "building ramify (cli/)"
  cargo build --release --quiet --manifest-path "$HOME_DIR/cli/Cargo.toml" || die "the ramify build failed"
  say "building ram (Ram Code)"
  cargo build --release --quiet --manifest-path "$HOME_DIR/Cargo.toml" || die "the ram build failed"
fi

mkdir -p "$BIN_DIR" || die "cannot create $BIN_DIR"
for name in $NAMES; do
  link="$BIN_DIR/$name"
  if [ -e "$link" ] && [ ! -L "$link" ] && [ "$FORCE" != 1 ]; then
    die "$link exists and is not a symlink; pass --force to replace it"
  fi
  rm -f "$link"
  ln -s "$HOME_DIR/bin/$name" "$link" || die "cannot link $link"
  say "linked $link -> $HOME_DIR/bin/$name"
done

missing=""
for tool in curl lsof python3 rsync; do
  command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
[ -n "$missing" ] && warn "ramify also needs:$missing"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR is not on PATH. Add this line to your shell profile:"
     printf '  export PATH="%s:$PATH"\n' "$BIN_DIR" >&2 ;;
esac

cat <<EOF

Done. Run \`ram\` in a configured checkout, or \`ramify setup\` to configure one.
Plugins, once per machine:
  Claude Code:  /plugin marketplace add $HOME_DIR
                /plugin install ramify@ramify
  Codex:        codex plugin marketplace add $HOME_DIR
EOF
