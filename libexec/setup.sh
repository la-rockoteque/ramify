#!/usr/bin/env bash
# Drafts a .ramify.conf from what the repo already shows: its main branch, the branch prefix its
# history uses, where its worktrees already live, what is gitignored but needed, its install, its
# compose containers, and the dev servers it can detect. A draft, not a verdict — the `setup`
# skill reviews it with a human.
#
# Usage: setup.sh <primary checkout> [--print | --force] [--local]
#   --print  write to stdout only    --force  overwrite an existing config
#   --local  write .claude/ramify.conf (personal, usually gitignored) instead of .ramify.conf
set -uo pipefail

P="$1"; shift
mode=write; target="$P/.ramify.conf"
for a in "$@"; do
  case "$a" in
    --print) mode=print ;;
    --force) mode=force ;;
    --local) target="$P/.claude/ramify.conf" ;;
  esac
done
name="$(basename "$P")"
g() { git -C "$P" "$@" 2>/dev/null; }

main="$(g symbolic-ref --short refs/remotes/origin/HEAD | sed 's#^origin/##')"
[ -n "$main" ] || main="$(g rev-parse --abbrev-ref HEAD)"; [ -n "$main" ] || main=main

# The prefix most branches share (story/, feat/, …), if one covers at least a third of them.
# A ticket key (IN-3/) names one ticket, not a convention: those branches do not count.
prefix="$(g for-each-ref --format='%(refname:lstrip=2)' refs/heads refs/remotes/origin \
  | sed 's#^origin/##' | grep -v -e '^HEAD$' -e "^$main\$" | grep -Ev '^[A-Z][A-Z0-9]*-[0-9]+/' | sort -u \
  | awk -F/ 'NF>1{c[$1"/"]++} {n++} END{for(k in c) if(c[k]*3>=n && c[k]>m){m=c[k]; b=k} print b}')"

# Where the linked worktrees already are; ../<repo>-wt otherwise.
wtdir="$(g worktree list --porcelain | awk '/^worktree /{print $2}' | tail -n +2 \
  | grep -v '/\.claude/worktrees/' | xargs -n1 dirname 2>/dev/null | sort | uniq -c | sort -rn | awk 'NR==1{print $2}')"
if [ -n "$wtdir" ]; then
  wtdir="$(python3 -c 'import os,sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$wtdir" "$P")"
else
  wtdir="../$name-wt"
fi

# Ports other projects on this machine already hold, as lo:hi ranges. A service base P holds
# P..P+RAMIFY_MAX_SLOT; a shared port holds itself. The repo being drafted does not count.
REGISTRY="${RAMIFY_REGISTRY:-${CLAUDE_CONFIG_DIR:-$HOME/.claude}/ramify/projects}"
span="${RAMIFY_MAX_SLOT:-5}"; held=""; self="$(cd "$P" && pwd -P)"
if [ -f "$REGISTRY" ]; then
  while read -r proj; do
    [ -n "$proj" ] && [ "$(cd "$proj" 2>/dev/null && pwd -P)" != "$self" ] || continue
    for f in "$proj/.ramify.conf" "$proj/.claude/ramify.conf"; do
      [ -f "$f" ] || continue
      # Assignments as bash reads them: drop the indent and any `export`.
      conf="$(sed -e 's/^[[:space:]]*//' -e 's/^export[[:space:]][[:space:]]*//' "$f")"
      max="$(sed -n 's/^RAMIFY_MAX_SLOT=["'\'']\{0,1\}\([0-9][0-9]*\).*/\1/p' <<<"$conf" | tail -1)"
      for p in $(sed -n 's/^[A-Za-z0-9_]*_shared_port=["'\'']\{0,1\}\([0-9][0-9]*\).*/\1/p' <<<"$conf"); do
        held="$held $p:$p"
      done
      for p in $(grep -v '_shared_port=' <<<"$conf" | sed -n 's/^[A-Za-z0-9_]*_port=["'\'']\{0,1\}\([0-9][0-9]*\).*/\1/p'); do
        held="$held $p:$((p + ${max:-5}))"
      done
      break
    done
  done <"$REGISTRY"
fi
# pick_port <base> <span>: the first base, in steps of 10, whose range overlaps nothing held.
# Sets $picked and holds its range, so no later service gets it.
pick_port() {
  local p="$1" s="$2" r lo hi free
  while :; do
    free=1
    for r in $held; do
      lo="${r%:*}"; hi="${r#*:}"
      [ "$p" -le "$hi" ] && [ $((p + s)) -ge "$lo" ] && { free=0; break; }
    done
    [ "$free" = 1 ] && break
    p=$((p + 10))
  done
  held="$held $p:$((p + s))"; picked="$p"
}

# The Node and uv packages. One python pass reads every package.json. It prints one line per
# finding, fields split by \037, and the loops below turn the lines into config:
#   install <dir> <command> <ready>        one per install root (lockfile, workspace or packageManager)
#   svc <name> <dir> <ui|other> <port> <cmd>   a dev server
#   sb <name> <dir> <cmd>                  its Storybook
#   fixed <dir>                            a NestJS app that listens on a literal port
#   mobile <dir> <notes>                   a React Native or Expo app
#   orchestrator <dir> <script>            a script that runs other packages (turbo, nx, …)
#   pkg <dir>                              every package, for the env file scan
read -r -d '' detect_py <<'PY'
import json, os, re, sys
P = sys.argv[1]
LOCKS = [("bun.lock", "bun"), ("bun.lockb", "bun"), ("pnpm-lock.yaml", "pnpm"),
         ("yarn.lock", "yarn"), ("package-lock.json", "npm")]
# A script that runs other packages' scripts. Those packages are listed on their own.
ORCHESTRATOR = re.compile(r"\b(turbo|nx|lerna|concurrently)\b|--workspaces?\b|--filter\b|(^|\s)-r(\s|$)")

def load(d):
    try:
        with open(os.path.join(d, "package.json")) as f:
            pj = json.load(f)
        return pj if isinstance(pj, dict) else None
    except Exception:
        return None

def table(pj, key):
    v = pj.get(key)
    return v if isinstance(v, dict) else {}

def deps(pj):
    return {**table(pj, "dependencies"), **table(pj, "devDependencies")}

def rel(d):
    r = os.path.relpath(d, P)
    return "" if r == "." else r

def has(d, *files):
    return any(os.path.isfile(os.path.join(d, f)) for f in files)

def is_root(d):
    pj = load(d) or {}
    return (has(d, *(l for l, _ in LOCKS), "pnpm-workspace.yaml")
            or "packageManager" in pj or "workspaces" in pj)

def install_root(d):
    cur = d
    while True:
        if is_root(cur):
            return cur
        if os.path.abspath(cur) == os.path.abspath(P):
            return d
        cur = os.path.dirname(cur)

def manager(root):
    pm = str((load(root) or {}).get("packageManager", "")).split("@")[0]
    if pm in ("bun", "pnpm", "yarn", "npm"):
        return pm
    for l, m in LOCKS:
        if has(root, l):
            return m
    return "pnpm" if has(root, "pnpm-workspace.yaml") else "npm"

def run(pm, script, args=""):
    if not args:
        return f"{pm} run {script}"
    return f"npm run {script} -- {args}" if pm == "npm" else f"{pm} run {script} {args}"

def word(name, text):
    # The command word itself: vite, not vite-node or scripts/next-step.js.
    return re.search(r"(^|[\s;&|(])" + name + r"(\s|$)", text) is not None

def subdirs(d):
    try:
        return sorted(os.path.join(d, e) for e in os.listdir(d)
                      if not e.startswith(".") and e != "node_modules" and os.path.isdir(os.path.join(d, e)))
    except OSError:
        return []

top = subdirs(P)
pkgs = [d for d in [P] + top + [s for d in top for s in subdirs(d)] if load(d) is not None]
pkgs.sort(key=lambda d: (rel(d).count("/") + (rel(d) != ""), rel(d)))

roots, bins, members, out, names, installs = [], {}, {}, [], set(), []
ui_named = False
for d in pkgs:
    pj, r = load(d), rel(d)
    scripts = {k: v for k, v in table(pj, "scripts").items() if isinstance(v, str)}
    dd = deps(pj)
    root = install_root(d)
    pm = manager(root)
    members.setdefault(root, []).append(dd)
    out.append(f"pkg\x1f{r}")
    if root not in roots:
        roots.append(root)
    if "react-native" in dd or "expo" in dd or (os.path.isdir(os.path.join(d, "ios")) and os.path.isdir(os.path.join(d, "android"))):
        runs = [run(pm, s) for s in ("ios", "android") if s in scripts]
        checks = [run(pm, s) for s in ("test", "lint") if s in scripts]
        notes = f"Mobile app in {r or 'the repo root'}, no QA stack."
        if runs:
            notes += " Run it: " + ", ".join(runs) + "."
        if checks:
            notes += " Check it: " + ", ".join(checks) + "."
        out.append(f"mobile\x1f{r}\x1f{notes}")
        continue
    if "@nestjs/core" in dd:
        try:
            main_ts = open(os.path.join(d, "src", "main.ts")).read()
        except Exception:
            main_ts = ""
        if re.search(r"\.listen\(\s*\d", main_ts):
            out.append(f"fixed\x1f{r}")
            continue
    script = next((s for s in ("dev", "start:dev", "start", "serve") if s in scripts), None)
    if script is None:
        continue
    t = scripts[script]
    if ORCHESTRATOR.search(t):
        out.append(f"orchestrator\x1f{r}\x1f{script}")
        continue
    if word("next", t):
        kind, cmd, port, b = "ui", run(pm, script, "-p $PORT"), 3000, "next"
    elif word("vite", t) or word("astro", t):
        b = "vite" if word("vite", t) else "astro"
        kind, cmd, port = "ui", run(pm, script, "--port $PORT"), 5173
    elif word("webpack serve", t) or word("webpack-dev-server", t):
        b = "webpack-dev-server" if word("webpack-dev-server", t) else "webpack"
        kind, cmd, port = "ui", run(pm, script, "--port $PORT"), 8080
    elif word("react-scripts start", t):
        kind, cmd, port, b = "ui", "BROWSER=none PORT=$PORT " + run(pm, script), 3000, "react-scripts"
    else:
        kind, cmd, port, b = "other", "PORT=$PORT " + run(pm, script), 3000, None
    if b and root not in bins:
        bins[root] = b
    if kind == "ui" and not ui_named:
        name, ui_named = "web", True
    else:
        name = re.sub(r"[^a-z0-9]", "_", (r or "app").lower())
        name = "_" + name if name[0].isdigit() else name
        if name in names or name == "web":
            name = name + "_" + script.replace(":", "_")
    names.add(name)
    out.append(f"svc\x1f{name}\x1f{r}\x1f{kind}\x1f{port}\x1f{cmd}")
    if "storybook" in scripts:
        sb = "storybook" if name == "web" else name + "_storybook"
        out.append(f"sb\x1f{sb}\x1f{r}\x1f{run(pm, 'storybook', '-p $PORT --no-open')}")

for root in roots:
    # A root with no lockfile, no workspace and no dependency has nothing to install, and npm
    # would write no marker for the hook to wait on.
    if not is_root(root) and not any(members.get(root, [])):
        continue
    pm, r = manager(root), rel(root)
    if pm == "bun":
        cmd = "bun install"
        first = sorted(k for dd in members.get(root, []) for k in dd)
        if root in bins:
            ready = "node_modules/.bin/" + bins[root]
        else:
            ready = f"node_modules/{first[0]}/package.json" if first else ""
    elif pm == "pnpm":
        cmd, ready = "pnpm install", "node_modules/.modules.yaml"
    elif pm == "yarn":
        cmd, ready = "yarn install --frozen-lockfile", "node_modules/.yarn-integrity"
    else:
        cmd = "npm ci" if has(root, "package-lock.json") else "npm install"
        ready = "node_modules/.package-lock.json"
    installs.append(f"install\x1f{r}\x1f{cmd}\x1f{ready}")
for d in [P] + top:
    if has(d, "uv.lock"):
        installs.append(f"install\x1f{rel(d)}\x1fuv sync\x1f.venv/pyvenv.cfg")
print("\n".join(installs + out))
PY
detected="$(python3 -c "$detect_py" "$P")" \
  || echo "ramify: package detection failed; the draft lists no install and no Node service" >&2

services=""; body=""; bootstrap=""; ready=""; webdirs=""; notes=""; pkgdirs=""
# sq <text>: the text, safe inside a single-quoted config value.
sq() { printf '%s' "$1" | sed "s/'/'\\\\''/g"; }
add() { services="$services $1"; body="$body"$'\n'"$2"$'\n'; }
sep=$'\037'   # not whitespace, so read keeps an empty field (the repo root's dir)
while IFS="$sep" read -r dir cmd rd; do
  if [ -n "$dir" ]; then bootstrap="${bootstrap:+$bootstrap && }(cd \"$dir\" && $cmd)"
  else bootstrap="${bootstrap:+$bootstrap && }$cmd"; fi
  # The installs run in turn, so the last marker shows that all of them are done.
  [ -z "$rd" ] || ready="${dir:+$dir/}$rd"
done <<EOF
$(printf '%s\n' "$detected" | grep '^install' | cut -d"$sep" -f2-)
EOF
while IFS="$sep" read -r kind a b c d e; do
  case "$kind" in
    svc)
      pick_port "$d" "$span"
      webdirs="$webdirs ${b:+$b/}"
      add "$a" "${a}_port=$picked
${a}_dir='$(sq "$b")'
${a}_health=/
${a}_cmd='$(sq "$e")'" ;;
    sb)
      pick_port 6006 "$span"
      add "$a" "${a}_port=$picked
${a}_dir='$(sq "$b")'
${a}_health=/index.json
${a}_cmd='$(sq "$c")'
# Boots last and nothing waits on it; RAMIFY_SKIP=$a leaves it out.
${a}_optional=1" ;;
    fixed)
      body="$body"$'\n'"# ${a:-.}: NestJS listens on a fixed port in src/main.ts (port not configurable), so it is not a service."$'\n' ;;
    orchestrator)
      body="$body"$'\n'"# ${a:-.}: the $b script runs other packages, so it is not a service. They are listed on their own."$'\n' ;;
    mobile)
      notes="${notes:+$notes }$b" ;;
    pkg)
      pkgdirs="$pkgdirs ${a:-.}" ;;
  esac
done <<EOF
$(printf '%s\n' "$detected" | grep -v '^install')
EOF

# Gitignored, untracked paths a fresh worktree needs and git will not bring. An ignored .env that
# a .env.example stands for is copied once it exists, so it is listed even when absent.
copy=""
keep() { case " $copy " in *" $1 "*) ;; *) g check-ignore -q "$1" && copy="$copy $1" ;; esac; }
for c in .claude .envrc .vscode/settings.json; do [ -e "$P/$c" ] && keep "$c"; done
for d in . $pkgdirs; do
  pre="${d#.}"; pre="${pre:+$pre/}"
  for f in "$P/$pre".env "$P/$pre".env.*; do
    [ -f "$f" ] || continue
    case "$f" in *.example|*.sample|*.template) continue ;; esac
    keep "$pre$(basename "$f")"
  done
  [ -f "$P/$pre.env.example" ] && { keep "$pre.env"; keep "$pre.env.local"; }
done
copy="${copy# }"

# The compose file at the root, else one or two levels down (infra/, packages/db/).
compose=""; containers=""
for f in "$P"/docker-compose.y*ml "$P"/compose.y*ml "$P"/*/docker-compose.y*ml "$P"/*/compose.y*ml \
         "$P"/*/*/docker-compose.y*ml "$P"/*/*/compose.y*ml; do
  [ -f "$f" ] || continue
  case "$f" in */node_modules/*) continue ;; esac
  compose="${f#"$P"/}"; break
done
[ -n "$compose" ] && containers="$(sed -n 's/^[[:space:]]*container_name:[[:space:]]*["'\'']\{0,1\}\([^"'\'' ]*\).*/\1/p' "$P/$compose" | tr '\n' ' ' | sed 's/ $//')"
if [ -n "$compose" ] && [ -z "$containers" ]; then
  compose_block="# $compose names no container_name. Give each service one and list them here:
# RAMIFY_COMPOSE_FILE='$(sq "$compose")'
# RAMIFY_CONTAINERS=\"\""
else
  compose_block="RAMIFY_COMPOSE_FILE='$(sq "$compose")'
RAMIFY_CONTAINERS=\"$containers\""
fi

# ASP.NET Core: a Web SDK project is an API. It gets a shared instance, because a branch that does
# not touch the backend builds the same API as main — and that build is the slow part.
for cs in $(cd "$P" && git ls-files '*.csproj' 2>/dev/null); do
  grep -q 'Sdk="Microsoft.NET.Sdk.Web"' "$P/$cs" || continue
  case "$cs" in *[Tt]est*) continue ;; esac
  proj="$(dirname "$cs")"
  shared_when="^($(printf '%s' "$webdirs docs/" | tr ' ' '\n' | grep . | sed 's#[.]#\\.#g' | paste -sd'|' -))"
  pick_port 5000 "$span"; api_port="$picked"
  pick_port 5100 0; api_shared="$picked"
  add api "# Slot N → $api_port+N. Slot 0 is never used, and a port someone else holds skips the slot.
api_port=$api_port
# Check it: the path that answers once the API really serves, not once it logs « listening ».
api_health=/health
api_timeout=180
# --urls, not ASPNETCORE_URLS: the launch profile overrides the environment variable.
api_cmd='DOTNET_WATCH_RESTART_ON_RUDE_EDIT=1 dotnet watch --project $proj --non-interactive run --urls http://localhost:\$PORT'
api_shared_port=$api_shared
api_shared_cmd='dotnet run --project $proj --urls http://localhost:\$PORT'
# While every changed path matches, the branch reuses the shared API and skips api_prepare.
api_shared_when='$shared_when'
# api_prepare='dotnet ef database update --project <Infrastructure> --startup-project $proj'"
  # The web servers proxy to it; say where, so the draft is one edit from working.
  body="$(printf '%s' "$body" | sed -E "/storybook_cmd=/!s#^([a-z_]+)_cmd='#\1_cmd='VITE_API_TARGET=http://localhost:\$API_PORT #")"
  # Proves the web server reaches the API through its proxy; set the path to a real endpoint.
  body="$body"$'\n'"# web_smoke='/api/health 200|401'"$'\n'
  services="api$services"   # first, so later services can read $API_PORT
  break
done
services="$(printf '%s' "$services" | tr ' ' '\n' | grep . | awk '!s[$0]++' | paste -sd' ' -)"

qa_plan=""
for d in docs/stories docs/specs docs/qa qa; do
  [ -d "$P/$d" ] && { qa_plan="$d/*/<slug>/qa-plan.md"; break; }
done
if [ -n "$notes" ]; then
  qa_notes="# Examples. Replace them with the repo's own.
# RAMIFY_QA_RESULTS='docs/stories/*/<slug>/qa-results.md'
RAMIFY_QA_NOTES='$(sq "$notes")'"
else
  qa_notes="# Examples. Replace them with the repo's own.
# RAMIFY_QA_RESULTS='docs/stories/*/<slug>/qa-results.md'
# RAMIFY_QA_NOTES='Seed demo data with ./scripts/seed.sh before list screens.'"
fi

workflow_config="$(python3 "$(dirname "$0")/repo-workflows.py" config "${RAMIFY_SETUP_ROOT:-$P}")" || exit 1

out="# ramify — $name. Sourced as bash: single-quote commands so \$PORT and \$<SERVICE>_PORT
# expand when the service starts, not now. Drafted by 'ramify setup'; review every line.

# ── Branching ──
# 'ramify new <slug>' cuts $prefix<slug> from origin/$main into $wtdir/<slug>.
RAMIFY_MAIN=$main
RAMIFY_BRANCH_PREFIX=$prefix
RAMIFY_WORKTREES=$wtdir
# Gitignored paths every new worktree needs; git does not bring them.
RAMIFY_COPY=\"$copy\"
RAMIFY_BOOTSTRAP='$bootstrap'
# Exists once the bootstrap is done; the worktree hook waits for it before 'up'.
RAMIFY_READY=$ready
$workflow_config
# Uncomment to let a branch that is behind main be pushed.
# RAMIFY_GATE_BEHIND=0

# ── QA recipe ──
# The checklist ask-for-qa hands over. <slug> is the worktree name.
RAMIFY_QA_PLAN='$qa_plan'
$qa_notes

# ── Shared containers (never stopped by ramify) ──
$compose_block

# ── Services, started in this order ──
RAMIFY_SERVICES=\"$services\"
$body"
[ -n "$services" ] || out="$out
# No dev server detected. Leave RAMIFY_SERVICES empty for branching only, or add a service:
# RAMIFY_SERVICES=\"app\"
# app_port=8000                 # slot N → 8000+N
# app_dir=                      # relative to the worktree
# app_health=/healthz
# app_cmd='python -m uvicorn main:app --port \$PORT'
"

if [ "$mode" = print ]; then printf '%s' "$out"; exit 0; fi
[ -f "$target" ] && [ "$mode" != force ] && { echo "ramify: $target exists — --force to overwrite, --print to compare" >&2; exit 1; }
mkdir -p "$(dirname "$target")"
printf '%s' "$out" >"$target"
echo "wrote $target"
echo "services: ${services:-(none detected)}   main: $main   prefix: ${prefix:-(none)}   worktrees: $wtdir"
