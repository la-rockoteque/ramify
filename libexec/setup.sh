#!/usr/bin/env bash
# Drafts a .ramify.conf from what the repo already shows: its main branch, the branch prefix its
# history uses, where its worktrees already live, what is gitignored but needed, its compose
# containers, and the dev servers it can detect. A draft, not a verdict — the `setup` skill
# reviews it with a human.
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
prefix="$(g for-each-ref --format='%(refname:lstrip=2)' refs/heads refs/remotes/origin \
  | sed 's#^origin/##' | grep -v -e '^HEAD$' -e "^$main\$" | sort -u \
  | awk -F/ 'NF>1{c[$1"/"]++} {n++} END{for(k in c) if(c[k]*3>=n && c[k]>m){m=c[k]; b=k} print b}')"

# Where the linked worktrees already are; ../<repo>-wt otherwise.
wtdir="$(g worktree list --porcelain | awk '/^worktree /{print $2}' | tail -n +2 \
  | grep -v '/\.claude/worktrees/' | xargs -n1 dirname 2>/dev/null | sort | uniq -c | sort -rn | awk 'NR==1{print $2}')"
if [ -n "$wtdir" ]; then
  wtdir="$(python3 -c 'import os,sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$wtdir" "$P")"
else
  wtdir="../$name-wt"
fi

# Gitignored paths a fresh worktree needs and git will not bring.
copy=""
for c in .claude .env .env.local .envrc .vscode/settings.json; do
  [ -e "$P/$c" ] && g check-ignore -q "$c" && copy="$copy $c"
done
copy="${copy# }"

compose=""; containers=""
for f in docker-compose.yml docker-compose.yaml compose.yml compose.yaml; do
  [ -f "$P/$f" ] && { compose="$f"; break; }
done
[ -n "$compose" ] && containers="$(sed -n 's/^[[:space:]]*container_name:[[:space:]]*["'\'']\{0,1\}\([^"'\'' ]*\).*/\1/p' "$P/$compose" | tr '\n' ' ' | sed 's/ $//')"

services=""; body=""; bootstrap=""; ready=""; webdirs=""
add() { services="$services $1"; body="$body"$'\n'"$2"$'\n'; }

# Node dev servers, at the root or up to two levels down (apps/web, packages/ui).
for pj in "$P/package.json" "$P"/*/package.json "$P"/*/*/package.json; do
  [ -f "$pj" ] || continue
  case "$pj" in */node_modules/*) continue ;; esac
  dir="$(dirname "$pj")"; rel="${dir#"$P"}"; rel="${rel#/}"
  scripts="$(python3 -c 'import json,sys; print(" ".join(json.load(open(sys.argv[1])).get("scripts",{}).keys()))' "$pj" 2>/dev/null)"
  deps="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(" ".join({**d.get("dependencies",{}),**d.get("devDependencies",{})}.keys()))' "$pj" 2>/dev/null)"
  case " $scripts " in *" dev "*) ;; *) continue ;; esac
  pm=npm; lock="node_modules/.package-lock.json"
  [ -f "$dir/pnpm-lock.yaml" ] && { pm=pnpm; lock="node_modules/.modules.yaml"; }
  [ -f "$dir/yarn.lock" ] && { pm=yarn; lock="node_modules/.yarn-integrity"; }
  bootstrap="${bootstrap:+$bootstrap && }(cd \"${rel:-.}\" && $pm install)"
  ready="${ready:-${rel:+$rel/}$lock}"
  webdirs="$webdirs ${rel:+$rel/}"
  # The first dev server is `web`; others are named after their directory.
  svc=web; case " $services " in *" web "*) svc="$(printf '%s' "${rel:-root}" | tr -c 'a-z0-9\n' '_' | sed 's/^[0-9]/_&/')" ;; esac
  if [[ " $deps " == *" next "* ]]; then port=3000; flag="-p"; else port=5173; flag="--port"; fi
  run="$pm run dev"; [ "$pm" = npm ] && run="npm run dev --"
  add "$svc" "${svc}_port=$port
${svc}_dir=${rel}
${svc}_health=/
${svc}_cmd='$run $flag \$PORT'"
  if [[ " $scripts " == *" storybook "* ]]; then
    sb=storybook; [ "$svc" = web ] || sb="${svc}_storybook"
    run="$pm run storybook"; [ "$pm" = npm ] && run="npm run storybook --"
    add "$sb" "${sb}_port=6006
${sb}_dir=${rel}
${sb}_health=/index.json
${sb}_cmd='$run -p \$PORT --no-open'
# Boots last and nothing waits on it; RAMIFY_SKIP=$sb leaves it out.
${sb}_optional=1"
  fi
done

# ASP.NET Core: a Web SDK project is an API. It gets a shared instance, because a branch that does
# not touch the backend builds the same API as main — and that build is the slow part.
for cs in $(cd "$P" && git ls-files '*.csproj' 2>/dev/null); do
  grep -q 'Sdk="Microsoft.NET.Sdk.Web"' "$P/$cs" || continue
  case "$cs" in *[Tt]est*) continue ;; esac
  proj="$(dirname "$cs")"
  shared_when="^($(printf '%s' "$webdirs docs/" | tr ' ' '\n' | grep . | sed 's#[.]#\\.#g' | paste -sd'|' -))"
  add api "# Slot N → 5000+N. Slot 0 is never used, and a port someone else holds skips the slot.
api_port=5000
# Check it: the path that answers once the API really serves, not once it logs « listening ».
api_health=/health
api_timeout=180
# --urls, not ASPNETCORE_URLS: the launch profile overrides the environment variable.
api_cmd='DOTNET_WATCH_RESTART_ON_RUDE_EDIT=1 dotnet watch --project $proj --non-interactive run --urls http://localhost:\$PORT'
api_shared_port=5100
api_shared_cmd='dotnet run --project $proj --urls http://localhost:\$PORT'
# While every changed path matches, the branch reuses the shared API and skips api_prepare.
api_shared_when='$shared_when'
# api_prepare='dotnet ef database update --project <Infrastructure> --startup-project $proj'"
  # The web servers proxy to it; say where, so the draft is one edit from working.
  body="$(printf '%s' "$body" | sed -E "s#^([a-z_]+)_cmd='(npm|pnpm|yarn) run dev#\1_cmd='VITE_API_TARGET=http://localhost:\$API_PORT \2 run dev#")"
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
# Uncomment to let a branch that is behind main be pushed.
# RAMIFY_GATE_BEHIND=0

# ── QA recipe ──
# The checklist ask-for-qa hands over. <slug> is the worktree name.
RAMIFY_QA_PLAN='$qa_plan'
# RAMIFY_QA_RESULTS='docs/stories/*/<slug>/qa-results.md'
# RAMIFY_QA_NOTES='Seed demo data with ./scripts/seed.sh before list screens.'

# ── Shared containers (never stopped by ramify) ──
RAMIFY_COMPOSE_FILE=$compose
RAMIFY_CONTAINERS=\"$containers\"

# ── Services, started in this order ──
RAMIFY_SERVICES=\"$services\"
$body"
[ -n "$services" ] || out="$out
# Nothing detected. A service is five lines:
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
