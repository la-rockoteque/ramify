#!/usr/bin/env bash
# End-to-end check on a throwaway repo: new → up (shared + private) → card → down → cleanup →
# prune → write-set → hooks. Services are `python3 -m http.server`, so nothing but git, curl,
# lsof and python3 is needed. Uses ports 17000-17300.
set -uo pipefail

RAMIFY="$(cd "$(dirname "$0")/.." && pwd)/bin/ramify"
HOOK="$(cd "$(dirname "$0")/.." && pwd)/hooks/ramify-hook"
T="$(cd "$(mktemp -d)" && pwd -P)"
export RAMIFY_STATE_DIR="$T/state" RAMIFY_REGISTRY="$T/registry" RAMIFY_AUTOSTART=0 RAMIFY_POPUP=0 WF_POLL=0.2 RAMIFY_RUNS_DIR="$T/runs" RAMIFY_RUN_POLL=0.2
# A fake docker: prune --apply must never reach the machine's real images. It logs each call.
mkdir -p "$T/bin"
printf '#!/bin/sh\necho "$*" >>"%s/docker.log"\n' "$T" >"$T/bin/docker"; chmod +x "$T/bin/docker"
export PATH="$T/bin:$PATH"
fails=0

teardown() { [ -n "${KEEP:-}" ] && { echo "kept $T"; return; }
  for p in $(seq 17000 17010) $(seq 17100 17100) $(seq 17200 17210); do
    lsof -ti tcp:"$p" -sTCP:LISTEN 2>/dev/null | xargs kill 2>/dev/null
  done
  rm -rf "$T"
}
trap teardown EXIT

ok()   { echo "ok    $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
check() { local name="$1"; shift; if "$@" >/dev/null 2>&1; then ok "$name"; else fail "$name"; fi; }
answers() { curl -sf -o /dev/null --max-time 2 "http://localhost:$1/"; }

# macOS ships bash 3.2 as /bin/bash, and a shebang or a `bash -c` can land there. Every script
# must at least parse under it.
if [ -x /bin/bash ] && /bin/bash -c '[ "${BASH_VERSINFO[0]}" -lt 4 ]'; then
  parses_on_old_bash() { local f; for f in $(grep -rlE '^#!.*bash' "$(dirname "$RAMIFY")/.." --exclude-dir=.git); do /bin/bash -n "$f" || return 1; done; }
  check "every script parses under /bin/bash $(/bin/bash -c 'echo $BASH_VERSION')" parses_on_old_bash
fi

git init -q --bare -b main "$T/origin.git"
git clone -q "$T/origin.git" "$T/app" 2>/dev/null
cd "$T/app"
git config user.email t@t; git config user.name t
mkdir -p src web docs .claude/worktrees/leak
echo api >src/api.txt; echo web >web/index.html
cat >.ramify.conf <<'EOF'
RAMIFY_WORKTREES=../app-wt
RAMIFY_BRANCH_PREFIX=story/
RAMIFY_COPY=".claude"
RAMIFY_SERVICES="api web"
api_port=17000
api_health=/
api_cmd='exec python3 -m http.server $PORT'
api_shared_port=17100
api_shared_cmd='exec python3 -m http.server $PORT'
api_shared_when='^(web/|docs/)'
web_port=17200
web_dir=web
web_health=/
web_cmd='echo "api is $API_PORT" >api.txt; sh -c "sleep 9$PORT &"; exec python3 -m http.server $PORT'
web_smoke='/ 200'
EOF
printf '.claude/\n' >.gitignore
echo local >.claude/notes.md; echo nested >.claude/worktrees/leak/x
git add -A && git commit -qm init && git push -q origin main 2>/dev/null

# ── new ──
out="$("$RAMIFY" new feat 2>&1)"
WT="$T/app-wt/feat"
check "new cuts the worktree"                   test -d "$WT"
check "new names the branch with the prefix"    test "$(git -C "$WT" rev-parse --abbrev-ref HEAD)" = story/feat
check "new copies gitignored paths"             test -f "$WT/.claude/notes.md"
check "new skips nested worktrees"              test ! -e "$WT/.claude/worktrees"
check "new prints the hook marker"              grep -q "^ramify: worktree $WT\$" <<<"$out"
check "every command takes --help"              bash -c "for c in \$('$RAMIFY' --help | awk 'NR>1{print \$1}'); do '$RAMIFY' \$c --help | grep -q \"^usage: ramify \$c\" || exit 1; done"
check "new --help creates nothing"              bash -c "'$RAMIFY' new --help && [ ! -e '$T/app-wt/--help' ] && ! git show-ref -q refs/heads/story/--help"
check "new refuses a dash slug"                 bash -c "! '$RAMIFY' new --new && [ ! -e '$T/app-wt/--new' ]"
check "new prints the session label"            grep -qE '^/color (red|blue|green|yellow|purple|orange|pink|cyan)$' <<<"$out"

# ── up, frontend-only branch → shared api ──
echo change >"$WT/web/page.html"
RAMIFY_ROOT="$WT" "$RAMIFY" up >"$T/up1.log" 2>&1
ENV1="$RAMIFY_STATE_DIR/app/feat.env"
check "up claims slot 1"                        grep -qx SLOT=1 "$ENV1"
check "unchanged api is shared"                 grep -qx API_SHARED=1 "$ENV1"
check "shared api answers on its port"          answers 17100
check "web answers on its slot port"            answers 17201
check "later services see earlier ports"        grep -qx "api is 17100" "$WT/web/api.txt"
check "ping passes"                             env RAMIFY_ROOT="$WT" "$RAMIFY" ping
check "card shows the shared api"               bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' card | grep -q 'api shared'"

# ── a second tree that touches the api → private api ──
"$RAMIFY" new back >/dev/null 2>&1
WT2="$T/app-wt/back"
echo change >>"$WT2/src/api.txt"
RAMIFY_ROOT="$WT2" "$RAMIFY" up >"$T/up2.log" 2>&1
ENV2="$RAMIFY_STATE_DIR/app/back.env"
check "second tree gets slot 2"                 grep -qx SLOT=2 "$ENV2"
check "changed api is private"                  grep -qx API_SHARED=0 "$ENV2"
check "private api answers on slot port"        answers 17002
check "dash draws both tiles"                   bash -c "[ \$(RAMIFY_ROOT='$WT' '$RAMIFY' dash | grep -c '^┌') = 2 ]"
check "dash is plain through a pipe"            bash -c "! RAMIFY_ROOT='$WT' '$RAMIFY' dash | grep -q \$'\\e'"
check "dash paints ready green on a terminal"    bash -c "RAMIFY_ROOT='$WT' RAMIFY_COLOR=1 '$RAMIFY' dash | grep -q \$'\\e\\[32mready'"
check "dash borders take the project colour"     bash -c "RAMIFY_ROOT='$WT' RAMIFY_COLOR=1 '$RAMIFY' dash | grep -qE \$'^\\e\\[(3[3-6]|9[3-6])m┌'"
check "NO_COLOR wins"                            bash -c "! RAMIFY_ROOT='$WT' NO_COLOR=1 RAMIFY_COLOR=1 '$RAMIFY' dash | grep -q \$'\\e'"
check "dash --json lists both running stacks"   bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash --json | python3 -c 'import json,sys; p=json.load(sys.stdin)[0]; assert sorted(w[\"slot\"] for w in p[\"worktrees\"] if w[\"slot\"]) == [1, 2]; assert p[\"shared\"][0][\"up\"] is True'"
check "dash shows a private service's memory"  bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash | grep -qE 'api private +http://localhost:17002 +[0-9]+ MB'"
check "dash --json carries memory, null on shared" bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash --json | python3 -c 'import json,sys; p=json.load(sys.stdin)[0]; s=[x for w in p[\"worktrees\"] for x in w[\"services\"]]; assert all(isinstance(x[\"mb\"], int) for x in s if x[\"kind\"] != \"shared\"), s; assert all(x[\"mb\"] is None for x in s if x[\"kind\"] == \"shared\"), s; assert isinstance(p[\"shared\"][0][\"mb\"], int)'"

other="$T/other"; git init -q "$other"
check "dash works from a repo without a config"  bash -c "cd '$other' && [ \$(env -u RAMIFY_ROOT '$RAMIFY' dash | grep -c '^┌') = 2 ]"
check "watch-style dash works outside git"       bash -c "cd / && env -u RAMIFY_ROOT '$RAMIFY' dash | grep -q '^┌'"

plain="$T/plain"; git init -q "$plain"; printf 'RAMIFY_BRANCH_PREFIX=feat/\n' >"$plain/.ramify.conf"
RAMIFY_ROOT="$plain" "$RAMIFY" config -q
check "a branching-only project stays out of dash" bash -c "cd '$other' && [ \$(env -u RAMIFY_ROOT '$RAMIFY' dash | grep -c '^┌') = 2 ] && ! env -u RAMIFY_ROOT '$RAMIFY' dash | grep -q plain"
check "the registry records both repos"         bash -c "grep -qxF '$T/app' '$RAMIFY_REGISTRY' && grep -qxF '$plain' '$RAMIFY_REGISTRY'"
check "dash --json lists the branching-only repo" bash -c "cd / && env -u RAMIFY_ROOT '$RAMIFY' dash --json | python3 -c 'import json,sys; p={x[\"primary\"]: x for x in json.load(sys.stdin)}; assert p[\"$plain\"][\"stacks\"] is False and p[\"$T/app\"][\"stacks\"] is True'"
check "the stop hook ignores it"                 bash -c "[ -z \"\$(echo '{\"session_id\":\"p\"}' | RAMIFY_AUTOSTART=1 RAMIFY_ROOT='$plain' '$HOOK' stop)\" ] && [ ! -f '$RAMIFY_STATE_DIR/plain/plain.env' ]"

# ── an agent's Bash tool reads output through a pipe; up and new must not hold it open ──
check "a second up keeps the same slot"         bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' up >/dev/null 2>&1; grep -qx SLOT=1 '$ENV1' && [ ! -f '$RAMIFY_STATE_DIR/app/slots/3' ]"
check "up returns through a pipe"               bash -c "RAMIFY_ROOT='$WT' perl -e 'alarm 60; exec @ARGV' '$RAMIFY' up | cat"
check "new returns through a pipe"              bash -c "RAMIFY_AUTOSTART=1 RAMIFY_BOOTSTRAP='sleep 30' perl -e 'alarm 20; exec @ARGV' '$RAMIFY' new piped | cat"
git worktree remove --force "$T/app-wt/piped" 2>/dev/null; git branch -D -q story/piped 2>/dev/null

# ── the guard in front of every signal, checked without sending one ──
# The sed script goes in as an argument: bash 3.2 garbles a double-quoted one nested in "$(…)".
GUARD_PARTS='/^TAG_NS=/,/^}/p; /^BUILD_SERVER_RE=/p; /^tagged_groups() {/,/^}/p'
guard() { # pgid → exit 0 when ramify would signal it
  bash -c '
    RAMIFY_PROJECT=app STATE_DIR="$RAMIFY_STATE_DIR/app"
    eval "$(sed -n "$3" "$1")"
    our_group "$2"' _ "$RAMIFY" "$1" "$GUARD_PARTS"
}
WEB_PGID="$(cat "$RAMIFY_STATE_DIR/app/feat.web.pid")"
check "the guard accepts a tagged service group" guard "$WEB_PGID"
refuses() { ! guard "$1"; }
for g in 1 0 -1 '' abc "$(ps -o pgid= -p $$ | tr -d ' ')" "$(ps -o pgid= -p "$PPID" | tr -d ' ')"; do
  check "the guard refuses pgid '$g'"           refuses "$g"
done
refuses_elsewhere() { RAMIFY_STATE_DIR=/elsewhere refuses "$1"; }
check "another state dir's tag is not ours"     refuses_elsewhere "$WEB_PGID"

# ── strays: a group no pid file names is found by the tag in its environment ──
# What a second `up` overwriting the pid file leaves behind: a pid that names nothing. NEVER a
# low pid such as 1 — `kill -- -1` is every process of the user.
true & DEAD=$!; wait "$DEAD"
echo "$DEAD" >"$RAMIFY_STATE_DIR/app/feat.web.pid"
check "dash --json lists the stray"             bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash --json | python3 -c 'import json,sys; s=json.load(sys.stdin)[0][\"strays\"]; assert [(x[\"worktree\"], x[\"service\"]) for x in s] == [(\"feat\", \"web\")], s'"
check "dash names the stray"                    bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash | grep -q '^STRAY  feat web'"
"$RAMIFY" cleanup >"$T/cleanup-stray.log" 2>&1
check "cleanup kills the stray"                 bash -c "! curl -sf -o /dev/null --max-time 2 http://localhost:17201/ && grep -q 'stray .*feat web' '$T/cleanup-stray.log'"
check "cleanup kills the stray's children"      bash -c "! pgrep -f 'sleep [9]17201'"
check "cleanup keeps the shared api"            answers 17100
check "dash --json carries the build servers"   bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash --json | python3 -c 'import json,sys; assert isinstance(json.load(sys.stdin)[0][\"build_servers\"], list)'"
check "dash draws the build servers line"       bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash | grep -q '^build servers'"

# ── one up per worktree: a live lock turns the second one away, a dead one does not ──
LOCK="$RAMIFY_STATE_DIR/app/feat.up.lock"
bash -c 'sleep 60; :' ramify-up-stand-in & LOCKER=$!   # a live holder whose command names ramify
mkdir "$LOCK"; echo "$LOCKER" >"$LOCK/pid"
check "a second up leaves the first one alone"  bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' up | grep -q 'already running' && ! curl -sf -o /dev/null --max-time 2 http://localhost:17201/"
kill "$LOCKER" 2>/dev/null; wait "$LOCKER" 2>/dev/null
sleep 60 & REUSED=$!                                    # a pid the system gave to another program
echo "$REUSED" >"$LOCK/pid"
check "a lock whose pid runs something else is stale" bash -c "! RAMIFY_ROOT='$WT' '$RAMIFY' up | grep -q 'already running'"
kill "$REUSED" 2>/dev/null; wait "$REUSED" 2>/dev/null
RAMIFY_ROOT="$WT" "$RAMIFY" down >/dev/null 2>&1
mkdir -p "$LOCK"; echo "$LOCKER" >"$LOCK/pid"           # its holder is dead now
RAMIFY_ROOT="$WT" "$RAMIFY" up >/dev/null 2>&1
check "a dead lock does not block up"           answers 17201
check "up drops its lock on the way out"        test ! -e "$RAMIFY_STATE_DIR/app/feat.up.lock"
check "up after a stray keeps slot 1"           grep -qx SLOT=1 "$ENV1"

# ── a service that never answers: up records why, the JSON carries it, down clears it ──
"$RAMIFY" new broken >/dev/null 2>&1
WT3="$T/app-wt/broken"
printf '%s\n' "web_cmd='echo \"boom: no such module\" >&2; exit 1'" web_timeout=2 >>"$WT3/.ramify.conf"   # the tree's own config wins
RAMIFY_ROOT="$WT3" "$RAMIFY" up >/dev/null 2>&1
check "a failed service is recorded with its log" bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash --json | python3 -c 'import json,sys; w=[w for w in json.load(sys.stdin)[0][\"worktrees\"] if w[\"name\"]==\"broken\"][0]; e=w[\"errors\"][0]; assert e[\"service\"]==\"web\" and \"boom\" in open(e[\"log\"]).read()'"
RAMIFY_ROOT="$WT3" "$RAMIFY" down >/dev/null 2>&1
check "down clears the recorded failure"        test ! -f "$RAMIFY_STATE_DIR/app/broken.errors"
git worktree remove --force "$WT3"; git branch -D -q story/broken

# ── <svc>_when: a service starts only once the branch touches what it shows ──
"$RAMIFY" new lean >/dev/null 2>&1
WT4="$T/app-wt/lean"
printf '%s\n' "web_when='^web/'" >>"$WT4/.ramify.conf"
RAMIFY_ROOT="$WT4" "$RAMIFY" up >/dev/null 2>&1
check "a _when service stays out of an unrelated branch" bash -c "! grep -q '^WEB_PORT=' '$RAMIFY_STATE_DIR/app/lean.env'"
RAMIFY_ROOT="$WT4" "$RAMIFY" down >/dev/null 2>&1
echo change >"$WT4/web/changed.txt"
RAMIFY_ROOT="$WT4" "$RAMIFY" up >/dev/null 2>&1
check "a _when service starts once a match changes" grep -q '^WEB_PORT=' "$RAMIFY_STATE_DIR/app/lean.env"
RAMIFY_ROOT="$WT4" "$RAMIFY" down >/dev/null 2>&1
git worktree remove --force "$WT4"; git branch -D -q story/lean

# ── a compiler server a build starts inside a service is shared: down spares it, it is no stray ──
"$RAMIFY" new comp >/dev/null 2>&1
WT5="$T/app-wt/comp"
printf '%s\n' "web_cmd='python3 -c \"import time; time.sleep(120)\" VBCSCompiler.dll & echo \$! >\"$T/compiler.pid\"; exec python3 -m http.server \$PORT'" >>"$WT5/.ramify.conf"
RAMIFY_ROOT="$WT5" "$RAMIFY" up >/dev/null 2>&1
COMP="$(cat "$T/compiler.pid" 2>/dev/null)"
check "the stand-in compiler shares the service's group" bash -c "[ \"\$(ps -o pgid= -p '$COMP' | tr -d ' ')\" = \"\$(cat '$RAMIFY_STATE_DIR/app/comp.web.pid')\" ]"
RAMIFY_ROOT="$WT5" "$RAMIFY" down >/dev/null 2>&1
check "down stops the service"                  bash -c "! curl -sf -o /dev/null --max-time 2 http://localhost:17203/"
check "down spares the compiler server"         kill -0 "$COMP"
check "a compiler server is never a stray"      bash -c "RAMIFY_ROOT='$WT5' '$RAMIFY' dash --json | python3 -c 'import json,sys; assert json.load(sys.stdin)[0][\"strays\"] == []'"
[[ "$COMP" =~ ^[0-9]+$ ]] && [ "$COMP" -gt 1 ] && kill "$COMP" 2>/dev/null
git worktree remove --force "$WT5"; git branch -D -q story/comp

# ── status line: the pinned worktree and its branch, nothing outside ramify ──
mkdir -p "$RAMIFY_STATE_DIR/sessions"; printf '%s\n' "$WT" >"$RAMIFY_STATE_DIR/sessions/sl.root"
check "statusline follows the session's pin"     bash -c "[ \"\$(printf '{\"session_id\":\"sl\",\"workspace\":{\"current_dir\":\"$T/app\"}}' | '$RAMIFY' statusline)\" = '𖣂 feat ⎇ story/feat · 2/2 up' ]"
check "statusline --json gives each port"      bash -c "printf '{\"session_id\":\"sl\"}' | '$RAMIFY' statusline --json | python3 -c 'import json,sys; p=json.load(sys.stdin)[\"ports\"]; assert p == {\"api\": 17100, \"web\": 17201}, p'"
check "statusline shows the primary's branch"    bash -c "[ \"\$(printf '{\"session_id\":\"none\",\"workspace\":{\"current_dir\":\"$T/app\"}}' | '$RAMIFY' statusline)\" = '𖣂 main' ]"
check "statusline is silent outside ramify"      bash -c "[ -z \"\$(printf '{\"session_id\":\"none\",\"workspace\":{\"current_dir\":\"$other\"}}' | '$RAMIFY' statusline)\" ]"
rm -f "$RAMIFY_STATE_DIR/sessions/sl.root"

# ── a stack from before tags: no tag, but its pid file and its port agree, so down stops it ──
perl -e 'setpgrp(0, 0); exec @ARGV' python3 -m http.server 17208 >/dev/null 2>&1 & LEGACY=$!
printf 'API_PORT=17208\nAPI_SHARED=0\n' >>"$ENV1"; echo "$LEGACY" >"$RAMIFY_STATE_DIR/app/feat.api.pid"
sleep 1

# ── down leaves shared things alone ──
RAMIFY_ROOT="$WT" "$RAMIFY" down >/dev/null 2>&1
sleep 1
check "down stops the tree's web"               bash -c "! curl -sf -o /dev/null --max-time 2 http://localhost:17201/"
check "down stops the web's grandchildren"      bash -c "! pgrep -f 'sleep [9]17201'"
check "down keeps the shared api"               answers 17100
check "down releases the slot"                  test ! -f "$RAMIFY_STATE_DIR/app/slots/1"
check "ping fails once down"                    bash -c "! RAMIFY_ROOT='$WT' '$RAMIFY' ping"
check "down stops an untagged stack its pid file and port agree on" bash -c "! curl -sf -o /dev/null --max-time 2 http://localhost:17208/"

# ── cleanup: a worktree deleted under a running stack ──
# An unrelated program now holds a port that a dead worktree's state still names: not ours.
perl -e 'setpgrp(0, 0); exec @ARGV' python3 -m http.server 17207 >/dev/null 2>&1 & OTHER=$!
printf 'WEB_PORT=17207\nWEB_SHARED=0\n' >"$RAMIFY_STATE_DIR/app/ghost.env"
sleep 1
git worktree remove --force "$WT2"
"$RAMIFY" cleanup >"$T/cleanup.log" 2>&1
sleep 1
check "cleanup stops the deleted tree's api"    bash -c "! curl -sf -o /dev/null --max-time 2 http://localhost:17002/"
check "cleanup releases its slot"               test ! -f "$RAMIFY_STATE_DIR/app/slots/2"
check "cleanup keeps the shared api"            answers 17100
check "cleanup leaves a listener that is not ours" bash -c "curl -sf -o /dev/null --max-time 2 http://localhost:17207/ && grep -q 'left pid .* alone on :17207' '$T/cleanup.log'"
kill "$OTHER" 2>/dev/null; wait "$OTHER" 2>/dev/null
"$RAMIFY" shared-down >/dev/null 2>&1

# ── prune: a merged branch (GitHub merge commit) ──
git -C "$WT" add -A && git -C "$WT" commit -qm feat
git merge -q --no-ff story/feat -m "Merge pull request #1 from someone/story/feat" 2>/dev/null
git push -q origin main 2>/dev/null
check "prune dry run lists the merged branch"   bash -c "'$RAMIFY' prune | grep -q 'prune  story/feat'"
check "prune dry run deletes nothing"           test -d "$WT"
check "prune --all runs from anywhere, every repo" bash -c "cd / && env -u RAMIFY_ROOT '$RAMIFY' prune --all | grep -q 'prune  story/feat' && cd / && env -u RAMIFY_ROOT '$RAMIFY' prune --all | grep -q '━━ plain ━━'"
git worktree add -q --detach "$T/app-detached" main
check "prune dry run lists a detached tree in main" bash -c "'$RAMIFY' prune | grep -q 'prune  app-detached (detached)'"
"$RAMIFY" prune --apply >/dev/null 2>&1
check "prune --apply removes the detached tree"  test ! -d "$T/app-detached"
check "prune --apply keeps compose containers and volumes" bash -c "grep -qx \"container prune -f --filter label!=com.docker.compose.project\" '$T/docker.log' && grep -q '^builder prune' '$T/docker.log' && ! grep -q volume '$T/docker.log'"
check "a repo with nothing running stays listed" bash -c "rm -rf '$RAMIFY_STATE_DIR'/*/primary; cd / && env -u RAMIFY_ROOT '$RAMIFY' dash --json | grep -qF '\"primary\":\"$T/app\"'"
check "prune --apply removes worktree + branch" bash -c "[ ! -d '$WT' ] && ! git show-ref -q refs/heads/story/feat"

# ── Jira: the key comes from the branch, complete and prune close the ticket ──
cat >"$T/jira.py" <<'PY'
import base64, json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
state = {"TM-7": "In Progress", "TM-8": "In Review", "TM-9": "Done", "TM-10": "In Progress"}
AUTH = "Basic " + base64.b64encode(b"qa@t:tok").decode()
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def reply(self, code, body=None):
        self.send_response(code); self.end_headers()
        if body is not None: self.wfile.write(json.dumps(body).encode())
    def key(self): return self.path.split("/")[5].split("?")[0]
    def do_GET(self):
        if self.headers.get("Authorization") != AUTH: return self.reply(401)
        k = self.key()
        if k not in state: return self.reply(404)
        if self.path.endswith("/transitions"):
            return self.reply(200, {"transitions": [{"id": "5", "name": "Review", "to": {"name": "In Review", "statusCategory": {"key": "indeterminate"}}},
                                                    {"id": "31", "name": "Done", "to": {"name": "Done", "statusCategory": {"key": "done"}}}]})
        cat = "done" if state[k] == "Done" else "indeterminate"
        self.reply(200, {"fields": {"status": {"name": state[k], "statusCategory": {"key": cat}}}})
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if body["transition"]["id"] == "31": state[self.key()] = "Done"
        open(sys.argv[2], "a").write(self.key() + "\n")
        self.reply(204)
HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY
python3 "$T/jira.py" 17350 "$T/closed.txt" & JIRA_PID=$!
printf 'RAMIFY_JIRA_URL=http://127.0.0.1:17350\nRAMIFY_JIRA_SYNC=0\n' >>.ramify.conf
git commit -qam jira && git push -q origin main 2>/dev/null
export RAMIFY_JIRA_EMAIL=qa@t RAMIFY_JIRA_TOKEN=tok
sleep 1
"$RAMIFY" new tm-7-login >/dev/null 2>&1
WT7="$T/app-wt/tm-7-login"
check "dash --json carries the branch's ticket"  bash -c "'$RAMIFY' dash --json | grep -q '\"ticket\":\"TM-7\"'"
echo x >"$WT7/x.txt"; git -C "$WT7" add x.txt; git -C "$WT7" commit -qm x
check "complete refuses unpushed commits"       bash -c "! '$RAMIFY' complete tm-7-login --yes && [ -d '$WT7' ] && [ ! -s '$T/closed.txt' ]"
git -C "$WT7" push -q origin story/tm-7-login 2>/dev/null
"$RAMIFY" complete tm-7-login --yes >"$T/complete.log" 2>&1
check "complete closes the ticket"              grep -qx TM-7 "$T/closed.txt"
check "complete removes worktree + branch"      bash -c "[ ! -d '$WT7' ] && ! git show-ref -q refs/heads/story/tm-7-login"
for k in 8 9; do git checkout -q -b "story/tm-$k-x" main; echo "$k" >"f$k"; git add "f$k"; git commit -qm "$k"; git checkout -q main
  git merge -q --no-ff "story/tm-$k-x" -m "Merge pull request #$k from someone/story/tm-$k-x"; done
git push -q origin main 2>/dev/null
check "prune dry run says what it would close"  bash -c "'$RAMIFY' prune | grep -q 'closes TM-8 (In Review)' && '$RAMIFY' prune | grep -q 'TM-9 already Done'"
"$RAMIFY" prune --apply >"$T/prune.log" 2>&1
check "prune --apply closes open tickets only"  bash -c "[ \"\$(sort '$T/closed.txt' | tr '\n' ' ')\" = 'TM-7 TM-8 ' ]"
check "jira refuses plain http off localhost"   bash -c "python3 '$(dirname "$RAMIFY")/../libexec/jira.py' check http://jira.example.com 2>&1 | grep -q 'must be an https'"
git checkout -q -b story/tm-10-kept main; echo 10 >f10; git add f10; git commit -qm 10; git checkout -q main
git merge -q --no-ff story/tm-10-kept -m "Merge pull request #10 from someone/story/tm-10-kept"; git push -q origin main 2>/dev/null
"$RAMIFY" cleanup >"$T/cleanup-jira.log" 2>&1
check "cleanup closes a merged branch's ticket" bash -c "grep -qx TM-10 '$T/closed.txt' && git show-ref -q refs/heads/story/tm-10-kept"
"$RAMIFY" cleanup >"$T/cleanup-jira2.log" 2>&1
check "cleanup leaves a done ticket alone"      bash -c "[ \$(grep -c TM-10 '$T/closed.txt') = 1 ]"
kill "$JIRA_PID" 2>/dev/null; unset RAMIFY_JIRA_EMAIL RAMIFY_JIRA_TOKEN

# ── write-set on a real branch ──
git checkout -q -b ws
echo x >src/new.txt
printf 'src/*\n' >"$T/ws-ok.txt"; printf 'docs/*\n' >"$T/ws-bad.txt"
check "write-set passes an allowed diff"        "$RAMIFY" write-set "$T/ws-ok.txt"
check "write-set refuses an outside diff"       bash -c "! '$RAMIFY' write-set '$T/ws-bad.txt'"
git checkout -q main; rm -f src/new.txt

# ── workflows ──
"$RAMIFY" new wfa --workflow feature --lane light --tag auth >"$T/wf-new.log" 2>&1
WFA="$T/app-wt/wfa"; ORDER="$WFA/.ramify/work/wfa/order.md"
WFPY="$(dirname "$RAMIFY")/../libexec/workflow.py"
check "new --workflow names the branch"         test "$(git -C "$WFA" rev-parse --abbrev-ref HEAD)" = feat/wfa
check "new --workflow writes the work order"    grep -qx 'branch: feat/wfa' "$ORDER"
check "a tag raises the lane to its floor"      grep -qx 'lane: standard' "$ORDER"
check "new refuses an unknown lane"             bash -c "! '$RAMIFY' new wfb --workflow bug --lane light && [ ! -e '$T/app-wt/wfb' ]"
check "new refuses an unknown workflow"         bash -c "! '$RAMIFY' new wfb --workflow nope && [ ! -e '$T/app-wt/wfb' ]"
cd "$WFA"
check "step names the next step"                bash -c "'$RAMIFY' step | grep -q '→ spec'"
check "steps are recorded in order"             bash -c "! '$RAMIFY' step done build"
check "a human gate needs --approved"           bash -c "! '$RAMIFY' step done spec"
# approvals: the agent asks, the human decides, the agent waits
SPEC="$WFA/.ramify/work/wfa/spec.md"
check "ask needs the file it shows"             bash -c "! '$RAMIFY' step ask spec"
printf '# Spec\n\n- AC1: it works\n' >"$SPEC"
check "ask refuses a step that is not a gate"   bash -c "! '$RAMIFY' step ask build"
check "approve refuses a step that is not next"     bash -c "! '$RAMIFY' approve build"
check "ask puts the gate up"                    bash -c "'$RAMIFY' step ask spec && grep -q 'step: spec' '$ORDER' && grep -q 'asked:' '$ORDER'"
check "status --line says it waits on you"      test "$(WF_ROOT="$WFA" WF_BRANCH=feat/wfa python3 "$WFPY" status --line)" = "feature/standard 0/8 ⏳ spec waits on you"
check "gates --json lists it with its text"     bash -c "'$RAMIFY' gates --json | python3 -c 'import json,sys; g=json.load(sys.stdin); assert len(g) == 1 and g[0][\"ask\"][\"step\"] == \"spec\", g; f=g[0][\"ask\"][\"files\"]; assert [x[\"path\"] for x in f] == [\".ramify/work/wfa/spec.md\", \".ramify/work/wfa/order.md\"] and \"AC1\" in f[0][\"text\"], f'"
check "wait times out without a decision"       bash -c "! '$RAMIFY' step wait spec --timeout 0"
check "a rejection needs a note"                bash -c "! '$RAMIFY' reject spec"
check "reject answers the request"              bash -c "'$RAMIFY' reject spec --note 'add an error state' && ! grep -q '^- {step: spec' '$ORDER'"
check "wait reports the rejection and its note" test "$("$RAMIFY" step wait spec --timeout 1)" = "rejected: add an error state"
check "a rejected request leaves gates"         test "$("$RAMIFY" gates --json)" = "[]"
"$RAMIFY" step ask spec >/dev/null
"$RAMIFY" step wait spec --timeout 10 >"$T/wait.out" & waiter=$!
"$RAMIFY" approve spec --note 'ship it' >/dev/null
wait "$waiter"; rc=$?
check "approve wakes a waiting agent"           bash -c "[ $rc = 0 ] && [ \"\$(cat '$T/wait.out')\" = 'approved: ship it' ]"
check "approve records the step"                bash -c "grep -q 'step: spec' '$ORDER' && grep -q 'note: ship it' '$ORDER' && ! grep -q '^ask:' '$ORDER'"
check "step na records n/a"                     bash -c "'$RAMIFY' step na qa-plan && grep -q 'step: qa-plan.*na: true' '$ORDER'"
check "a lane is never lowered"                 bash -c "! '$RAMIFY' lane light"
check "status --line sums up the lane"          test "$(WF_ROOT="$WFA" WF_BRANCH=feat/wfa python3 "$WFPY" status --line)" = "feature/standard 2/8 → build"
check "dash --json carries the work order"      bash -c "'$RAMIFY' dash --json | python3 -c 'import json,sys; w={w[\"name\"]: w for w in json.load(sys.stdin)[0][\"worktrees\"]}; f=w[\"wfa\"][\"workflow\"]; assert f[\"next\"][\"step\"] == \"build\" and [s[\"state\"] for s in f[\"steps\"][:3]] == [\"done\", \"na\", \"next\"], f; assert w[\"app\"][\"workflow\"] is None'"
cp "$ORDER" "$ORDER.keep"; sed -i '' 's/^lane: .*/lane: zz/' "$ORDER" 2>/dev/null || sed -i 's/^lane: .*/lane: zz/' "$ORDER"
check "a broken order is a short line"          test "$(WF_ROOT="$WFA" WF_BRANCH=feat/wfa python3 "$WFPY" status --line)" = 'workflow: broken, see `ramify step`'
check "a broken order is an error in dash"      bash -c "'$RAMIFY' dash --json | python3 -c 'import json,sys; w={w[\"name\"]: w for w in json.load(sys.stdin)[0][\"worktrees\"]}; assert \"zz\" in w[\"wfa\"][\"workflow\"][\"error\"]'"
mv "$ORDER.keep" "$ORDER"
check "statusline shows the next step"          bash -c "printf '{\"session_id\":\"none\",\"workspace\":{\"current_dir\":\"$WFA\"}}' | '$RAMIFY' statusline | grep -q ' · feature/standard 2/8 → build\$'"
echo 'export RAMIFY_WORKFLOWS=0   # off here' >>"$WFA/.ramify.conf"
check "statusline honours workflows off"        bash -c "! printf '{\"session_id\":\"none\",\"workspace\":{\"current_dir\":\"$WFA\"}}' | '$RAMIFY' statusline | grep -q feature/"
git -C "$WFA" checkout -q .ramify.conf
check "the pr gate names what is missing"       bash -c "out=\$(WF_ROOT='$WFA' WF_BRANCH=feat/wfa python3 '$WFPY' gate pr); [ \$? = 1 ] && grep -q 'build, audit, qa, review' <<<\"\$out\""
mkdir -p .ramify && printf 'workflows: off\n' >.ramify/workflows.yml
check "a repo turns workflows off"              bash -c "'$RAMIFY' workflow | grep -qx 'workflows are off for this repo'"
printf 'workflows:\n  x: { lanes: { a: [nope] } }\nsteps: {}\n' >.ramify/workflows.yml
check "a lane naming an undefined step fails"   bash -c "'$RAMIFY' workflow 2>&1 | grep -q \"names step 'nope'\""
rm -f .ramify/workflows.yml
sed -i.bak '/^- {step/d' "$ORDER"
check "a hand-emptied steps: is not a traceback" bash -c "'$RAMIFY' step 2>&1 | grep -q '→ spec' && ! '$RAMIFY' step 2>&1 | grep -q Traceback"
mv "$ORDER.bak" "$ORDER"
check "step done needs a step"                  bash -c "'$RAMIFY' step done 2>&1 | grep -q 'usage: ramify step done'"
check "workflow keeps internal verbs out"       bash -c "! '$RAMIFY' workflow gate pr"
cd "$T/app"
check "gate passes a branch with no work order" env WF_ROOT="$T/app" WF_BRANCH=main python3 "$WFPY" gate pr
check "new refuses a slug with a slash"         bash -c "! '$RAMIFY' new a/b --workflow tweak && [ ! -e '$T/app-wt/a' ]"
check "an unknown tag is reported"              bash -c "'$RAMIFY' new wfc --workflow feature --lane light --tag authh | grep -q \"'authh' sets no lane floor\""
mkdir -p .ramify/work/merged && printf -- '---\nworkflow: tweak\nlane: default\nbranch: chore/merged\n---\n' >.ramify/work/merged/order.md
git add .ramify && git commit -qm merged && git push -q origin main 2>/dev/null
check "a merged slug is refused before the cut" bash -c "'$RAMIFY' new merged --workflow tweak 2>&1 | grep -q 'pick another slug' && [ ! -e '$T/app-wt/merged' ]"

# ── run: the agent hands a script over, the human runs or blocks it, the agent waits ──
printf 'echo "ran in $PWD"\necho oops >&2\nexit 3\n' >"$T/job.sh"
check "run ask needs a script"                  bash -c "! '$RAMIFY' run ask '$T/nope.sh'"
check "run ask freezes a copy"                  bash -c "cd '$WFA' && '$RAMIFY' run ask '$T/job.sh' --note 'try it' | grep -q 'waits on the human'"
RUN="$(ls "$T/runs" | head -1)"
check "the copy is read-only, with its hash"    bash -c "[ ! -w '$T/runs/$RUN/script' ] && grep -q '\"sha256\": \"[0-9a-f]\\{64\\}\"' '$T/runs/$RUN/meta.json'"
check "run line names it for its worktree"      test "$("$RAMIFY" run line "$WFA")" = "▶ run $RUN waits on you"
check "run line skips another worktree"         test -z "$("$RAMIFY" run line "$T/elsewhere")"
check "runs lists it as pending"                bash -c "'$RAMIFY' runs | grep -q '$RUN  pending'"
check "runs --json carries the note"            bash -c "'$RAMIFY' runs --json | python3 -c 'import json,sys; r=json.load(sys.stdin); assert r[0][\"note\"] == \"try it\" and r[0][\"status\"] == \"pending\", r'"
check "run wait times out without a decision"   bash -c "! '$RAMIFY' run wait '$RUN' --timeout 0"
"$RAMIFY" run wait "$RUN" --timeout 10 >"$T/run-wait.out" & waiter=$!
"$RAMIFY" run go "$RUN" >"$T/run-go.out" 2>&1
wait "$waiter"; rc=$?
check "run go runs it in its directory"         bash -c "grep -q 'ran in $WFA' '$T/run-go.out' && grep -q oops '$T/runs/$RUN/output.log'"
check "run go wakes the agent with the output"  bash -c "[ $rc = 0 ] && head -1 '$T/run-wait.out' | grep -qx 'ran: exit 3' && grep -q oops '$T/run-wait.out'"
check "a run runs once"                         bash -c "! '$RAMIFY' run go '$RUN'"
check "a finished run leaves run line"          test -z "$("$RAMIFY" run line)"
"$RAMIFY" run ask "$T/job.sh" >/dev/null; sleep 1
RUN2="$(ls "$T/runs" | sort | tail -1)"
check "run block records the reason"            bash -c "'$RAMIFY' run block '$RUN2' --note 'not on prod' && [ \"\$('$RAMIFY' run wait '$RUN2')\" = 'blocked: not on prod' ]"
check "a blocked run never runs"                bash -c "! '$RAMIFY' run go '$RUN2' && [ ! -e '$T/runs/$RUN2/output.log' ]"
"$RAMIFY" run ask "$T/job.sh" >/dev/null; sleep 1
RUN3="$(ls "$T/runs" | sort | tail -1)"
chmod u+w "$T/runs/$RUN3/script"; echo 'echo sneaked in' >>"$T/runs/$RUN3/script"
check "run go refuses a changed copy"           bash -c "! '$RAMIFY' run go '$RUN3' && [ ! -e '$T/runs/$RUN3/output.log' ]"
check "the refusal blocks it and says why"      bash -c "'$RAMIFY' run wait '$RUN3' | grep -q 'blocked: the frozen script changed'"

# ── hooks ──
bare="$(mktemp -d)"; git -C "$bare" init -q
check "hooks are silent without a config"       bash -c "[ -z \"\$(echo '{\"tool_input\":{\"command\":\"git push\"},\"cwd\":\"$bare\"}' | '$HOOK' behind)\" ]"
rm -rf "$bare"
out="$(printf '{"session_id":"s1","cwd":"%s","tool_input":{"command":"ramify new x"},"tool_response":{"stdout":"ramify: worktree %s\\n"}}' "$T/app" "$T/app" | RAMIFY_AUTOSTART=1 "$HOOK" worktree-add)"
check "worktree-add hook pins the session"      test "$(cat "$RAMIFY_STATE_DIR/sessions/s1.root")" = "$T/app"
check "worktree-add hook hands over"            grep -q 'ramify:ask-for-qa' <<<"$out"
git checkout -q -b behind; git checkout -q main
echo more >>src/api.txt; git commit -qam more; git push -q origin main 2>/dev/null; git checkout -q behind
out="$(printf '{"cwd":"%s","tool_input":{"command":"git push -u origin behind"}}' "$T/app" | "$HOOK" behind)"
check "behind hook denies a stale push"         grep -q '"permissionDecision": "deny"' <<<"$out"
prg() { printf '{"cwd":"%s","tool_input":{"command":"%s"}}' "$1" "$2" | "$HOOK" pr-gate; }
dg() { printf '{"cwd":"%s","tool_input":{"command":"%s"}}' "$WFA" "$1" | "$HOOK" decide-guard; }
check "decide-guard keeps approve from the agent" grep -q '"permissionDecision": "deny"' <<<"$(dg 'ramify approve spec')"
check "decide-guard sees a reject after &&"     grep -q '"permissionDecision": "deny"' <<<"$(dg 'cd x && RAMIFY_ROOT=y ramify reject spec --note no')"
check "decide-guard lets ask and wait through"  test -z "$(dg 'ramify step ask spec; ramify step wait spec')"
check "decide-guard ignores a word in a message" test -z "$(dg 'git commit -m \\"approve the spec\\"')"
check "decide-guard keeps run go from the agent" grep -q 'Only the human runs or blocks' <<<"$(dg 'ramify run go 20261016-101010-1')"
check "decide-guard sees run.sh block"          grep -q '"permissionDecision": "deny"' <<<"$(dg 'X=1 /p/libexec/run.sh block 1 --note no')"
check "decide-guard lets run ask and wait through" test -z "$(dg 'ramify run ask s.sh && ramify run wait 1')"
# ── allow: plain ramify commands run without a prompt ──
al() { python3 -c 'import json,sys; print(json.dumps({"cwd": sys.argv[1], "tool_name": "Bash", "tool_input": {"command": sys.argv[2]}}))' "${AL_CWD:-$T/app}" "$1" | "$HOOK" allow; }
allowed() { local c; for c in "$@"; do grep -q '"permissionDecision": "allow"' <<<"$(al "$c")" || return 1; done; }
undecided() { local c; for c in "$@"; do [ -z "$(al "$c")" ] || return 1; done; }
NL='
'
check "allow lets ramify step through"          allowed 'ramify step'
check "allow takes cd and RAMIFY_ROOT to a worktree" allowed "cd $T/app && RAMIFY_ROOT=$T/app ramify step ask spec"
check "allow takes a pipe to head and 2>&1"     allowed 'ramify gates --json | head -5' 'ramify card 2>&1' 'ramify config 2>/dev/null | grep main'
check "allow leaves every prompted command alone" undecided 'ramify step done review --approved' 'ramify approve spec' \
  'ramify reject spec --note no' 'ramify run go 1' 'ramify run block 1' 'ramify delete x --yes' 'ramify complete x --yes' \
  'ramify step done review --approved=1' 'ramify step done review --approve' 'ramify prune --apply' 'ramify cleanup' \
  'ramify shared-down' 'ramify setup' 'ramify jira login' 'ramify ticket sync' 'ramify menu'
check "allow leaves a chain with another command alone" undecided 'ramify step && rm -rf x' 'ramify step; curl x' 'ramify step | sh'
check "allow leaves another ramify binary alone" undecided 'PATH=/tmp ramify step' './bin/ramify step' '/tmp/x/ramify step'
check "allow leaves substitution and redirection alone" undecided 'ramify step $(rm x)' 'ramify step `rm x`' \
  'ramify config <(rm x)' 'ramify config > f' 'ramify config >> f' 'ramify config < f' 'ramify card 12>&1' 'ramify card &>/dev/null'
check "allow leaves a comment before a newline alone" undecided "ramify step # note${NL}touch x" "ramify step#${NL}touch x" "ramify step${NL}#${NL}touch x"
check "allow leaves expansion alone"            undecided 'RAMIFY_X=--approved; ramify step done review $RAMIFY_X' \
  'ramify step done review ${X:---approved}' "ramify step done review \$'\\x2d-approved'" 'ramify step done review -[-]approved' \
  'ramify step done review ~' 'ramify step done review {a,b}'
check "allow leaves a RAMIFY_ prefix other than RAMIFY_ROOT alone" undecided "RAMIFY_BOOTSTRAP='curl x|sh' ramify new foo" 'RAMIFY_STATE_DIR=/tmp ramify up'
check "allow leaves a tree outside this repo alone" undecided 'cd /tmp && ramify step' 'RAMIFY_ROOT=/tmp ramify step' 'cd tmp && ramify step'
outside() { [ -z "$(AL_CWD=/tmp al 'ramify step')" ]; }
check "allow leaves a cwd outside this repo alone" outside
auto_off() { [ -z "$(RAMIFY_AUTO_ALLOW=0 al 'ramify step')" ]; }
check "RAMIFY_AUTO_ALLOW=0 turns allow off"     auto_off
check "allow keeps pipe filters off files"     undecided 'ramify gates | head -c 1 /etc/shadow' 'ramify gates | tail -f x' \
  'ramify gates | grep -f /etc/passwd' 'ramify gates | grep -r x /' 'ramify gates | grep x /etc/passwd' 'ramify gates | wc /etc/passwd' \
  'ramify gates | grep -ex /etc/passwd'
check "allow takes filter flags and a grep pattern" allowed 'ramify gates | tail -n 3' 'ramify gates | grep -i -c main' 'ramify gates | wc -l'
check "allow ignores other commands"            undecided 'ls' 'git status'
check "allow takes the plugin's own bin/ramify" allowed "$RAMIFY step"
check "allow survives a command shlex cannot parse" undecided "ramify step 'open"
out="$(prg "$WFA" 'gh pr create --fill')"
check "pr-gate refuses a PR with steps missing" bash -c "grep -q '\"permissionDecision\": \"deny\"' <<<'$out' && grep -q 'pr waits on: build, audit, qa, review' <<<'$out'"
check "pr-gate names the next skill"            grep -q 'Next: build — invoke ramify:build' <<<"$out"
check "pr-gate lets a draft through"            test -z "$(prg "$WFA" 'gh pr create --draft --fill')"
check "pr-gate ignores a branch with no order"  test -z "$(prg "$T/app" 'gh pr create --fill')"
check "pr-gate ignores other commands"          test -z "$(prg "$WFA" 'gh pr view 3')"
mkdir -p "$WFA/sub"
check "pr-gate holds from a subfolder"          grep -q '"permissionDecision": "deny"' <<<"$(prg "$WFA/sub" 'gh pr create --fill')"
check "pr-gate sees gh -R owner/repo"           grep -q '"permissionDecision": "deny"' <<<"$(prg "$WFA" 'gh -R o/r pr create --fill')"
check "pr-gate lets a multi-line draft through" test -z "$(prg "$WFA" 'gh pr create --title x \\\n  --draft')"
cp "$ORDER" "$ORDER.keep"; sed -i.bak 's/^lane: .*/lane: zz/' "$ORDER"
check "pr-gate lets a broken order through"     test -z "$(prg "$WFA" 'gh pr create --fill')"
mv "$ORDER.keep" "$ORDER"; rm -f "$ORDER.bak"
echo 'RAMIFY_GATE_WORKFLOW=0' >>"$WFA/.ramify.conf"
check "RAMIFY_GATE_WORKFLOW=0 turns it off"     test -z "$(prg "$WFA" 'gh pr create --fill')"
git -C "$WFA" checkout -q .ramify.conf
(cd "$WFA" && "$RAMIFY" step done build && "$RAMIFY" step done audit && "$RAMIFY" step done qa && "$RAMIFY" step done review --approved) >/dev/null
check "pr-gate opens once the steps are in"     test -z "$(prg "$WFA" 'gh pr ready')"
# The title checks from before session names: with names off, nothing changes.
export RAMIFY_SESSION_NAMES=off
out="$(printf '{"session_id":"none","source":"startup"}' | CLAUDE_PROJECT_DIR="$WFA" "$HOOK" session-start)"
check "session-start names the next step"       bash -c "grep -q '\"hookEventName\": \"SessionStart\"' <<<'$out' && grep -q '→ pr' <<<'$out'"
check "session-start is silent without workflows" test -z "$(printf '{"session_id":"none"}' | CLAUDE_PROJECT_DIR=/ "$HOOK" session-start)"
check "session-start names a worktree session"  grep -q "\"sessionTitle\": \"$(basename "$WFA")\"" <<<"$out"
ss() { printf '{"session_id":"none",%s}' "$2" | CLAUDE_PROJECT_DIR="$1" "$HOOK" session-start; }
check "session-start keeps a custom name"       bash -c "! grep -q sessionTitle <<<'$(ss "$WFA" '"source":"resume","session_title":"mine"')'"
check "session-start leaves /clear alone"       bash -c "! grep -q sessionTitle <<<'$(ss "$WFA" '"source":"clear"')'"
check "session-start leaves the primary unnamed" bash -c "! grep -q sessionTitle <<<'$(ss "$T/app" '"source":"startup"')'"
unset RAMIFY_SESSION_NAMES

# ── session names ──
NAMES="$(dirname "$RAMIFY")/../assets/names"
SN="$RAMIFY_STATE_DIR/sessions"
ns() { printf '{"session_id":"%s","source":"%s"%s}' "$1" "${3:-startup}" "${4:-}" | CLAUDE_PROJECT_DIR="$2" "$HOOK" session-start; }
se() { printf '{"session_id":"%s","reason":"%s"}' "$1" "$2" | CLAUDE_PROJECT_DIR=/ "$HOOK" session-end; }
named() { cat "$SN/$1.name" 2>/dev/null; }
out="$(ns s1 "$T/app")"
check "session-start writes a name from trees"  grep -qxF "$(named s1)" "$NAMES/trees.txt"
check "the name file holds one line"            test "$(wc -l <"$SN/s1.name" | tr -d ' ')" = 1
check "session-start titles the primary with the name" grep -q "\"sessionTitle\": \"$(named s1)\"" <<<"$out"
ns s2 "$T/app" >/dev/null
check "two sessions get two names"              bash -c "[ -n '$(named s2)' ] && [ '$(named s1)' != '$(named s2)' ]"
out="$(ns t1 "$WFA")"
check "session-start titles a worktree slug · name" grep -q "\"sessionTitle\": \"$(basename "$WFA") · $(named t1)\"" <<<"$out"
RAMIFY_SESSION_NAMES=birds ns b1 "$T/app" >/dev/null
check "RAMIFY_SESSION_NAMES picks a built-in theme" grep -qxF "$(named b1)" "$NAMES/birds.txt"
for s in w1 w2 w3; do RAMIFY_SESSION_NAMES="ash elm" ns "$s" "$T/app" >/dev/null; done
check "RAMIFY_SESSION_NAMES takes a word list"  grep -qxE 'ash|elm' <<<"$(named w1)"
check "a full theme adds -2"                    grep -qxE '(ash|elm)-2' <<<"$(named w3)"
out="$(RAMIFY_SESSION_NAMES=off ns o1 "$T/app")"
check "RAMIFY_SESSION_NAMES=off writes no name" bash -c "[ ! -e '$SN/o1.name' ] && ! grep -q sessionTitle <<<'$out'"
out="$(ns c1 "$WFA" resume ',"session_title":"mine"')"
check "a custom title keeps the name file"      bash -c "[ -s '$SN/c1.name' ] && ! grep -q sessionTitle <<<'$out'"
n1="$(named s1)"; se s1 other
check "session-end releases the name"           bash -c "[ ! -e '$SN/s1.name' ] && [ ! -e '$SN/held/$n1' ]"
n2="$(named s2)"; se s2 prompt_input_exit
check "prompt_input_exit releases the name too" bash -c "[ -n '$n2' ] && [ ! -e '$SN/s2.name' ] && [ ! -e '$SN/held/$n2' ]"
# A suffixed title: a random pick takes a bare word first, so only the reclaim gives these.
nt="$(named t1)"; se t1 prompt_input_exit
ns r1 "$WFA" resume ",\"session_title\":\"$(basename "$WFA") · $nt-3\"" >/dev/null
check "resume takes the name from a worktree title" bash -c "[ -n '$nt' ] && [ '$(named r1)' = '$nt-3' ]"
ns r2 "$T/app" resume ",\"session_title\":\"$n1-2\"" >/dev/null
check "resume takes the name from a primary title" test "$(named r2)" = "$n1-2"
ns r3 "$T/app" resume ",\"session_title\":\"$n1-2\"" >/dev/null
check "resume takes a new name when the title's name is held" bash -c "[ -n '$(named r3)' ] && [ '$(named r3)' != '$n1-2' ]"
ns r4 "$WFA" resume ",\"session_title\":\"$(basename "$WFA")\"" >/dev/null
check "resume does not take a slug title as a name" grep -qxF "$(named r4)" "$NAMES/trees.txt"
ns cl1 "$T/app" clear ",\"session_title\":\"$n1-4\"" >/dev/null
check "/clear takes the name from the title"    test "$(named cl1)" = "$n1-4"
echo st1 >"$SN/held/$n1-5"
ns st1 "$T/app" resume ",\"session_title\":\"$n1-5\"" >/dev/null
check "a resume takes back its own stale hold"  test "$(named st1)" = "$n1-5"
held="$(ls "$SN/held" | wc -l)"
ns "../../bad" "$T/app" >/dev/null
check "a session id with a slash gets no name"  bash -c "[ ! -e '$RAMIFY_STATE_DIR/bad.name' ] && [ \"\$(ls '$SN/held' | wc -l)\" = '$held' ]"
ns ml1 "$T/app" resume ",\"session_title\":\"../../x\\n$n1-6\"" >/dev/null
check "a multi-line title is not a name"        bash -c "[ ! -e '$RAMIFY_STATE_DIR/x' ] && [ \"\$(wc -l <'$SN/ml1.name' | tr -d ' ')\" = 1 ] && [ '$(named ml1)' != '$n1-6' ]"
RAMIFY_SESSION_NAMES=-n ns dn1 "$T/app" >/dev/null
check "a word that looks like a flag is a name" test "$(named dn1)" = "-n"
RAMIFY_SESSION_NAMES="/a ../b" ns e1 "$T/app" >/dev/null
check "a theme without a usable word names nothing" test ! -e "$SN/e1.name"
ns n1 / >/dev/null
check "no config, no name"                      test ! -e "$SN/n1.name"
sl() { printf '{"session_id":"%s","workspace":{"current_dir":"%s"}}' "$1" "$T/app" | "$RAMIFY" statusline "${2:-}"; }
check "statusline --json has the name"          grep -q "\"name\": \"$n1-2\"" <<<"$(sl r2 --json)"
check "statusline --json name is null without one" grep -q '"name": null' <<<"$(sl nobody --json)"
check "the plain status line starts with the name" grep -q "^𖣂 $n1-2 · $(git -C "$T/app" branch --show-current)" <<<"$(sl r2)"
touch "$T/notadir"
out="$(RAMIFY_STATE_DIR="$T/notadir/x" ns u1 "$T/app")"; rc=$?
check "an unwritable state dir names nothing and exits 0" bash -c "[ $rc = 0 ] && ! grep -q sessionTitle <<<'$out'"
themes_ok() { local f; for f in "$NAMES"/*.txt; do
  [ -s "$f" ] && ! grep -qvE '^[a-z][a-z-]*$' "$f" && [ -z "$(sort "$f" | uniq -d)" ] || return 1; done; }
check "every built-in theme has one word per line, no duplicates" themes_ok

# ── two clones with the same directory name keep their own state ──
# An old clone left in another folder must not share slots and pid files with the live one: its
# `cleanup` sees none of the live worktrees and would stop every stack.
for c in one two; do
  git clone -q "$T/origin.git" "$T/$c/twin" 2>/dev/null
  printf "RAMIFY_SERVICES=''\n" >"$T/$c/twin/.ramify.conf"
done
d1="$(RAMIFY_ROOT="$T/one/twin" "$RAMIFY" config get STATE_DIR)"
d2="$(RAMIFY_ROOT="$T/two/twin" "$RAMIFY" config get STATE_DIR)"
check "same-named clones get separate state"    test "$d1" != "$d2"
check "the first clone keeps the plain name"    test "$d1" = "$RAMIFY_STATE_DIR/twin"
check "the first clone stays the dir's primary" grep -qxF "$T/one/twin" "$d1/primary"
check "the second clone's state dir is stable"  test "$d2" = "$(RAMIFY_ROOT="$T/two/twin" "$RAMIFY" config get STATE_DIR)"
check "the status line reads the same dir"      bash -c "mkdir -p '$d2' && echo SLOT=4 >'$d2/twin.env' && echo '{\"workspace\":{\"current_dir\":\"$T/two/twin\"}}' | '$(dirname "$RAMIFY")/../libexec/statusline.sh' --json | grep -q '\"slot\": *\"*4'"
rm -rf "$T/one"
git clone -q "$T/origin.git" "$T/three/twin" 2>/dev/null; printf "RAMIFY_SERVICES=''\n" >"$T/three/twin/.ramify.conf"
check "a dir whose clone is gone is taken over" test "$(RAMIFY_ROOT="$T/three/twin" "$RAMIFY" config get STATE_DIR)" = "$d1"

# ── setup drafts a config from what the repo shows ──
S="$T/drafted"; git init -q -b trunk "$S"; mkdir -p "$S/ui"
echo '{"scripts":{"dev":"vite","storybook":"storybook dev"}}' >"$S/ui/package.json"
mkdir -p "$S/apps/site" "$S/ui/node_modules/dep"; echo '{"scripts":{"dev":"next dev"},"dependencies":{"next":"1"}}' >"$S/apps/site/package.json"
echo '{"scripts":{"dev":"x"}}' >"$S/ui/node_modules/dep/package.json"
printf 'services:\n  db:\n    container_name: drafted-db\n' >"$S/compose.yaml"
git -C "$S" commit -q --allow-empty -m init; for b in feat/a feat/b fix/c; do git -C "$S" branch "$b"; done
RAMIFY_ROOT="$S" "$RAMIFY" setup >/dev/null 2>&1
check "setup writes .ramify.conf"               test -f "$S/.ramify.conf"
check "setup detects dev servers two levels deep" grep -qx 'RAMIFY_SERVICES="web storybook apps_site"' "$S/.ramify.conf"
check "setup finds the branch prefix"           grep -qx 'RAMIFY_BRANCH_PREFIX=feat/' "$S/.ramify.conf"
check "setup finds the main branch"             grep -qx 'RAMIFY_MAIN=trunk' "$S/.ramify.conf"
check "setup finds the compose containers"      grep -qx 'RAMIFY_CONTAINERS="drafted-db"' "$S/.ramify.conf"
check "setup refuses to overwrite"              bash -c "! RAMIFY_ROOT='$S' '$RAMIFY' setup"
check "the draft loads"                         env RAMIFY_ROOT="$S" "$RAMIFY" config

# Each fixture is a throwaway repo; nothing is installed. `has` matches one whole line of a draft.
fx() { FX="$T/fx/$1"; rm -rf "$FX"; mkdir -p "$FX"; git init -q -b main "$FX"; git -C "$FX" commit -q --allow-empty -m init; }
draft() { RAMIFY_ROOT="$FX" "$RAMIFY" setup --print 2>/dev/null; }
has() { grep -qxF -- "$2" <<<"$1"; }
both() { has "$1" "$2" && has "$1" "$3"; }
lacks() { ! grep -q -- "$2" <<<"$1"; }
pj() { mkdir -p "$FX/${2:-.}"; printf '%s\n' "$1" >"$FX/${2:-.}/package.json"; }

fx bun; pj '{"scripts":{"dev":"vite"},"dependencies":{"react":"1"}}'; touch "$FX/bun.lock"; D="$(draft)"
check "setup installs a bun repo with bun and waits for a file bun writes" \
  both "$D" "RAMIFY_BOOTSTRAP='bun install'" 'RAMIFY_READY=node_modules/.bin/vite'
check "setup runs a bun dev server with bun"     has "$D" "web_cmd='bun run dev --port \$PORT'"
fx yarn; pj '{"scripts":{"dev":"vite"}}'; touch "$FX/yarn.lock"; D="$(draft)"
check "setup installs a yarn repo with a frozen lockfile" \
  both "$D" "RAMIFY_BOOTSTRAP='yarn install --frozen-lockfile'" 'RAMIFY_READY=node_modules/.yarn-integrity'
fx npm; pj '{"scripts":{"dev":"vite"}}'; touch "$FX/package-lock.json"; D="$(draft)"
check "setup installs an npm lockfile with npm ci" \
  both "$D" "RAMIFY_BOOTSTRAP='npm ci'" 'RAMIFY_READY=node_modules/.package-lock.json'
fx uv; touch "$FX/uv.lock"; D="$(draft)"
check "setup syncs a uv repo and waits for its venv" \
  both "$D" "RAMIFY_BOOTSTRAP='uv sync'" 'RAMIFY_READY=.venv/pyvenv.cfg'
fx pm; pj '{"packageManager":"bun@1.1.0","scripts":{"dev":"vite"}}'; touch "$FX/package-lock.json"; D="$(draft)"
check "setup takes packageManager over the lockfile" has "$D" "RAMIFY_BOOTSTRAP='bun install'"
fx ws; pj '{"workspaces":["packages/*"]}'; touch "$FX/yarn.lock"
pj '{"scripts":{"dev":"vite"}}' packages/a; pj '{"scripts":{"dev":"vite"}}' packages/b; D="$(draft)"
check "setup installs a workspace once at its root, without cd" has "$D" "RAMIFY_BOOTSTRAP='yarn install --frozen-lockfile'"
check "setup never gives two services the same base port" \
  test "$(grep -E '^[a-z_]+_port=' <<<"$D" | cut -d= -f2 | sort | uniq -d)" = ""
fx cra; pj '{"scripts":{"start":"react-scripts start"}}'; touch "$FX/yarn.lock"; D="$(draft)"
check "setup runs react-scripts with BROWSER=none and PORT" has "$D" "web_cmd='BROWSER=none PORT=\$PORT yarn run start'"
fx hono; pj '{"scripts":{"dev":"bun run src/index.ts"}}'; touch "$FX/bun.lock"; D="$(draft)"
check "setup passes PORT in the environment to a plain dev script" has "$D" "app_cmd='PORT=\$PORT bun run dev'"
fx nest; pj '{"scripts":{"dev":"vite"}}' ui; pj '{"scripts":{"start:dev":"nest start"},"dependencies":{"@nestjs/core":"1"}}' api
mkdir -p "$FX/api/src"; echo 'await app.listen(3000);' >"$FX/api/src/main.ts"; D="$(draft)"
nest_ok() { has "$D" 'RAMIFY_SERVICES="web"' && grep -q '^# api: .*port not configurable' <<<"$D"; }
check "setup leaves out a NestJS backend with a fixed port and says why" nest_ok
fx busy; pj '{"scripts":{"dev":"vite"}}'; mkdir -p "$T/fx/other"; printf 'web_port=5173\n' >"$T/fx/other/.ramify.conf"
echo "$T/fx/other" >"$T/fx/registry"; D="$(RAMIFY_REGISTRY="$T/fx/registry" draft)"; port="$(sed -n 's/^web_port=//p' <<<"$D")"
check "setup moves a base port off a range the registry holds" bash -c "[ -n '$port' ] && { [ '$port' -lt 5173 ] || [ '$port' -gt 5178 ]; }"
fx env; printf '.env\n.env.*\n!.env.example\n' >"$FX/.gitignore"; touch "$FX/.env.example" "$FX/.env.development" "$FX/.env.test" "$FX/.env.sample"
git -C "$FX" add .gitignore .env.example; git -C "$FX" commit -qm env; D="$(draft)"; copy="$(sed -n 's/^RAMIFY_COPY=//p' <<<"$D")"
check "setup copies an ignored .env that .env.example implies" grep -qx '\.env' <<<"$(tr -d '"' <<<"$copy" | tr ' ' '\n')"
check "setup copies ignored .env.* files that exist" bash -c 'grep -q "\.env\.development" <<<"$1" && grep -q "\.env\.test" <<<"$1"' _ "$copy"
check "setup does not copy an ignored .env.sample" lacks "$copy" sample
fx compose; mkdir -p "$FX/packages/db"; printf 'services:\n  db:\n    container_name: fx-db\n' >"$FX/packages/db/docker-compose.yml"; D="$(draft)"
check "setup finds a compose file two levels down" \
  both "$D" "RAMIFY_COMPOSE_FILE='packages/db/docker-compose.yml'" 'RAMIFY_CONTAINERS="fx-db"'
fx bare; printf 'services:\n  db:\n    image: x\n' >"$FX/compose.yaml"; D="$(draft)"
check "setup writes no compose file without containers" lacks "$D" '^RAMIFY_COMPOSE_FILE='
fx ticket; for b in IN-1/a IN-2/b IN-3/c feat/x; do git -C "$FX" branch "$b"; done; D="$(draft)"
check "setup skips ticket-key branch prefixes"   has "$D" "RAMIFY_BRANCH_PREFIX=feat/"
fx mobile; pj '{"scripts":{"ios":"expo run:ios","test":"jest","start":"expo start"},"dependencies":{"expo":"1"}}'; touch "$FX/bun.lock"; D="$(draft)"
mobile_ok() { has "$D" 'RAMIFY_SERVICES=""' && grep '^RAMIFY_QA_NOTES=' <<<"$D" | grep 'bun run ios' | grep -q 'bun run test'; }
check "setup drafts a mobile app as branching only with QA notes" mobile_ok
fx pnpm; pj '{"scripts":{"dev":"vite"}}'; touch "$FX/pnpm-workspace.yaml"; D="$(draft)"
check "setup installs a pnpm workspace with pnpm" has "$D" "RAMIFY_BOOTSTRAP='pnpm install'"
fx nolock; pj '{"scripts":{"dev":"vite"},"dependencies":{"vite":"1"}}'; D="$(draft)"
check "setup installs a repo without a lockfile with npm install" has "$D" "RAMIFY_BOOTSTRAP='npm install'"
fx thin; pj '{"scripts":{"lint":"x"}}'; pj '{"scripts":{"dev":"vite"},"dependencies":{"vite":"1"}}' web; touch "$FX/web/yarn.lock"; D="$(draft)"
check "setup skips an install with nothing to install and waits for the last one" \
  both "$D" "RAMIFY_BOOTSTRAP='(cd \"web\" && yarn install --frozen-lockfile)'" 'RAMIFY_READY=web/node_modules/.yarn-integrity'
fx bunapi; pj '{"workspaces":["apps/*"]}'; touch "$FX/bun.lock"; pj '{"scripts":{"dev":"bun run src/i.ts"},"dependencies":{"hono":"1"}}' apps/api; D="$(draft)"
check "setup waits for a dependency bun installs when no dev server bin is known" has "$D" 'RAMIFY_READY=node_modules/hono/package.json'
fx turbo; pj '{"workspaces":["apps/*"],"scripts":{"dev":"turbo run dev"}}'; touch "$FX/yarn.lock"; pj '{"scripts":{"dev":"next dev"}}' apps/web; D="$(draft)"
check "setup leaves out a root script that runs the other packages" has "$D" 'RAMIFY_SERVICES="web"'
fx words; pj '{"scripts":{"dev":"vite-node src/server.ts"}}'; D="$(draft)"
check "setup does not take vite-node for vite"   has "$D" "app_cmd='PORT=\$PORT npm run dev'"
fx broken; pj '{"scripts":{"dev":"vite"}}'; pj '{"dependencies":null,"scripts":{"dev":7}}' lib; mkdir -p "$FX/locked"; chmod 000 "$FX/locked"; D="$(draft)"; chmod 755 "$FX/locked"
check "setup survives a malformed package.json and an unreadable directory" has "$D" 'RAMIFY_SERVICES="web"'
fx "quo'te"; pj '{"scripts":{"ios":"x"},"dependencies":{"expo":"1"}}' "bob's app"
check "a draft with a quote in a path still loads" bash -c 'RAMIFY_ROOT="$1" "$2" setup >/dev/null 2>&1 && RAMIFY_ROOT="$1" "$2" config >/dev/null' _ "$FX" "$RAMIFY"
fx dotnet; mkdir -p "$FX/src/Api"; echo '<Project Sdk="Microsoft.NET.Sdk.Web"></Project>' >"$FX/src/Api/Api.csproj"; git -C "$FX" add src
pj '{"scripts":{"dev":"vite","storybook":"sb"}}' ui; touch "$FX/ui/bun.lock"
printf 'api_port=5000
api_shared_port=5100
sb_port=6006
' >"$T/fx/other/.ramify.conf"; D="$(RAMIFY_REGISTRY="$T/fx/registry" draft)"
dotnet_ok() { has "$D" 'api_port=5010' && has "$D" 'api_shared_port=5110' && has "$D" 'storybook_port=6016' \
  && has "$D" "web_cmd='VITE_API_TARGET=http://localhost:\$API_PORT bun run dev --port \$PORT'"; }
check "setup moves the API, its shared port and Storybook off held ports" dotnet_ok
fx empty; D="$(draft)"
check "setup says it found no dev server, not a library" \
  bash -c 'grep -q "^# No dev server detected" <<<"$1" && ! grep -q library <<<"$1"' _ "$D"
check "setup marks the QA examples as examples"  bash -c 'grep -B1 "^# RAMIFY_QA_RESULTS=" <<<"$1" | grep -q "^# Examples"' _ "$D"

echo
[ "$fails" = 0 ] && echo "smoke: all passed" || echo "smoke: $fails failed"
exit "$fails"
