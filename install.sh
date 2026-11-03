#!/usr/bin/env bash
# install.sh — install ram (Ram Code) and the ramify CLI it drives.
#
# From a checkout:   ./install.sh
# From the remote:   curl -fsSL https://raw.githubusercontent.com/la-rockoteque/ramify/main/install.sh | bash
#
# Piped, it downloads the latest GitHub Release for this machine: the repo tree with ram and
# ramify prebuilt. From a checkout, or with --from-source, it builds them with cargo instead.
# Either way it links bin/ram and bin/ramify into a directory on PATH. Run it again to update.
# `--uninstall` removes the links.
set -uo pipefail

REPO="${RAMIFY_REPO:-https://github.com/la-rockoteque/ramify.git}"
REF="${RAMIFY_REF:-main}"
RELEASES="${RAMIFY_RELEASES:-https://github.com/la-rockoteque/ramify/releases}"
VERSION="${RAMIFY_VERSION:-}"
BIN_DIR="${RAMIFY_BIN_DIR:-$HOME/.local/bin}"
HOME_DIR="${RAMIFY_HOME:-}"
BUILD=1 FORCE=0 UNINSTALL=0 SOURCE=0
NAMES="ram ramify"

usage() {
  cat <<EOF
Usage: install.sh [--version TAG] [--from-source] [--bin-dir DIR] [--home DIR] [--no-build]
                  [--force] [--uninstall]

  --version TAG  Install that release, such as v0.2.0 (default: the latest, or \$RAMIFY_VERSION)
  --from-source  Clone the source and build it with cargo instead of downloading a release
  --bin-dir DIR  Where to link ram and ramify (default: ~/.local/bin, or \$RAMIFY_BIN_DIR)
  --home DIR     Where ramify lives. Default: this script's checkout, else ~/.local/share/ramify.
                 A missing DIR gets a release, or a clone with --from-source. An existing
                 checkout is used as it is. Also \$RAMIFY_HOME
  --no-build     With a checkout, link only. The launchers build the binaries on first use
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
    --version) [ $# -ge 2 ] || die "--version needs a tag"; VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --from-source) SOURCE=1; shift ;;
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

DEFAULT_HOME="$HOME/.local/share/ramify"

# The mode: install from a checkout as it is, clone the source and build, or download a release.
if [ -z "$HOME_DIR" ] && [ -n "$here" ] && is_checkout "$here"; then
  HOME_DIR="$here" mode=checkout
else
  [ -n "$HOME_DIR" ] || HOME_DIR="$DEFAULT_HOME"
  if [ -f "$HOME_DIR/RELEASE" ] && [ "$SOURCE" = 0 ]; then mode=release
  elif [ ! -e "$HOME_DIR" ]; then
    if [ "$SOURCE" = 1 ]; then mode=clone; else mode=release; fi
  # The default home belongs to the installer: an earlier source install there is updated, or
  # replaced by a release. An explicit home that exists may be a working checkout: it is used
  # as it is, and the installer never pulls into it.
  elif [ "$HOME_DIR" = "$DEFAULT_HOME" ] && [ -d "$HOME_DIR/.git" ]; then
    if [ "$SOURCE" = 1 ]; then mode=clone; else mode=release; fi
  else mode=checkout
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
  [ "$mode" != checkout ] && [ -d "$HOME_DIR" ] && say "$HOME_DIR stays; remove it with: rm -rf \"$HOME_DIR\""
  exit 0
fi

# The release target of this machine, as the release workflow names its archives.
release_target() {
  local os arch
  case "$(uname -s)" in
    Darwin) os=apple-darwin ;;
    Linux) os=unknown-linux-gnu ;;
    *) return 1 ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64) arch=aarch64 ;;
    x86_64|amd64) arch=x86_64 ;;
    *) return 1 ;;
  esac
  printf '%s-%s\n' "$arch" "$os"
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  else sha256sum "$1" | cut -d' ' -f1
  fi
}

install_release() {
  local target asset url new old
  command -v curl >/dev/null 2>&1 || die "curl is required to download a release"
  command -v tar >/dev/null 2>&1 || die "tar is required to unpack a release"
  target="$(release_target)" \
    || die "no release is built for $(uname -s) $(uname -m); pass --from-source to build it"
  asset="ramify-$target.tar.gz"
  if [ -n "$VERSION" ]; then url="$RELEASES/download/$VERSION/$asset"
  else url="$RELEASES/latest/download/$asset"
  fi
  TMP_DIR="$(mktemp -d)" || die "cannot create a temporary directory"
  trap 'rm -rf "$TMP_DIR"' EXIT
  local tmp="$TMP_DIR"
  say "downloading $url"
  curl -fsSL -o "$tmp/$asset" "$url" || die "download failed: $url"
  curl -fsSL -o "$tmp/$asset.sha256" "$url.sha256" || die "download failed: $url.sha256"
  [ "$(sha256_of "$tmp/$asset")" = "$(cut -d' ' -f1 <"$tmp/$asset.sha256")" ] \
    || die "the checksum of $asset does not match; nothing was installed"
  mkdir -p "$(dirname "$HOME_DIR")" || die "cannot create $(dirname "$HOME_DIR")"
  new="$HOME_DIR.new.$$" old="$HOME_DIR.old.$$"
  rm -rf "$new" && mkdir "$new" || die "cannot create $new"
  tar -xzf "$tmp/$asset" -C "$new" || { rm -rf "$new"; die "cannot unpack $asset"; }
  if ! is_checkout "$new" || [ ! -x "$new/target/release/ram-code" ] || [ ! -x "$new/cli/target/release/ramify" ]; then
    rm -rf "$new"; die "$asset does not hold a ramify release"
  fi
  # Swap the new tree in, so a failed download never leaves a half install behind.
  if [ -e "$HOME_DIR" ]; then mv "$HOME_DIR" "$old" || { rm -rf "$new"; die "cannot move $HOME_DIR aside"; }; fi
  mv "$new" "$HOME_DIR" || { [ -e "$old" ] && mv "$old" "$HOME_DIR"; die "cannot install into $HOME_DIR"; }
  rm -rf "$old"
  say "installed $(cat "$HOME_DIR/RELEASE") for $target into $HOME_DIR"
}

case "$mode" in
  release) install_release; BUILD=0 ;;
  clone)
    command -v git >/dev/null 2>&1 || die "git is required"
    if [ -d "$HOME_DIR/.git" ]; then
      say "updating $HOME_DIR"
      git -C "$HOME_DIR" fetch --quiet origin "$REF" || die "git fetch failed in $HOME_DIR"
      git -C "$HOME_DIR" checkout --quiet "$REF" || die "git checkout $REF failed in $HOME_DIR"
      git -C "$HOME_DIR" merge --quiet --ff-only "origin/$REF" \
        || die "$HOME_DIR has local changes that block a fast-forward to origin/$REF"
    else
      say "cloning $REPO into $HOME_DIR"
      mkdir -p "$(dirname "$HOME_DIR")" || die "cannot create $(dirname "$HOME_DIR")"
      git clone --quiet --branch "$REF" "$REPO" "$HOME_DIR" || die "git clone failed"
    fi ;;
esac

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
