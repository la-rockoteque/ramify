#!/usr/bin/env bash
# End-to-end check on a throwaway repo: new → up (shared + private) → card → down → cleanup →
# prune → write-set → hooks. Services are `python3 -m http.server`, so nothing but git, curl,
# lsof and python3 is needed. Uses ports 17000-17300.
set -uo pipefail

RAMIFY="$(cd "$(dirname "$0")/.." && pwd)/bin/ramify"
HOOK="$(cd "$(dirname "$0")/.." && pwd)/hooks/ramify-hook"
T="$(cd "$(mktemp -d)" && pwd -P)"
export RAMIFY_STATE_DIR="$T/state" RAMIFY_REGISTRY="$T/registry" RAMIFY_AUTOSTART=0
fails=0

teardown() {
  for p in $(seq 17000 17010) $(seq 17100 17100) $(seq 17200 17210); do
    lsof -ti:"$p" 2>/dev/null | xargs kill 2>/dev/null
  done
  rm -rf "$T"
}
trap teardown EXIT

ok()   { echo "ok    $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
check() { local name="$1"; shift; if "$@" >/dev/null 2>&1; then ok "$name"; else fail "$name"; fi; }
answers() { curl -sf -o /dev/null --max-time 2 "http://localhost:$1/"; }

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
web_cmd='echo "api is $API_PORT" >api.txt; exec python3 -m http.server $PORT'
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
check "dash --json lists both running stacks"   bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash --json | python3 -c 'import json,sys; p=json.load(sys.stdin)[0]; assert sorted(w[\"slot\"] for w in p[\"worktrees\"] if w[\"slot\"]) == [1, 2]; assert p[\"shared\"][0][\"up\"] is True'"

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

# ── a service that never answers: up records why, the JSON carries it, down clears it ──
"$RAMIFY" new broken >/dev/null 2>&1
WT3="$T/app-wt/broken"
printf '%s\n' "web_cmd='echo \"boom: no such module\" >&2; exit 1'" web_timeout=2 >>"$WT3/.ramify.conf"   # the tree's own config wins
RAMIFY_ROOT="$WT3" "$RAMIFY" up >/dev/null 2>&1
check "a failed service is recorded with its log" bash -c "RAMIFY_ROOT='$WT' '$RAMIFY' dash --json | python3 -c 'import json,sys; w=[w for w in json.load(sys.stdin)[0][\"worktrees\"] if w[\"name\"]==\"broken\"][0]; e=w[\"errors\"][0]; assert e[\"service\"]==\"web\" and \"boom\" in open(e[\"log\"]).read()'"
RAMIFY_ROOT="$WT3" "$RAMIFY" down >/dev/null 2>&1
check "down clears the recorded failure"        test ! -f "$RAMIFY_STATE_DIR/app/broken.errors"
git worktree remove --force "$WT3"; git branch -D -q story/broken

# ── status line: the pinned worktree and its branch, nothing outside ramify ──
mkdir -p "$RAMIFY_STATE_DIR/sessions"; printf '%s\n' "$WT" >"$RAMIFY_STATE_DIR/sessions/sl.root"
check "statusline follows the session's pin"     bash -c "[ \"\$(printf '{\"session_id\":\"sl\",\"workspace\":{\"current_dir\":\"$T/app\"}}' | '$RAMIFY' statusline)\" = '𖣂 feat ⎇ story/feat · 2/2 up' ]"
check "statusline shows the primary's branch"    bash -c "[ \"\$(printf '{\"session_id\":\"none\",\"workspace\":{\"current_dir\":\"$T/app\"}}' | '$RAMIFY' statusline)\" = '𖣂 main' ]"
check "statusline is silent outside ramify"      bash -c "[ -z \"\$(printf '{\"session_id\":\"none\",\"workspace\":{\"current_dir\":\"$other\"}}' | '$RAMIFY' statusline)\" ]"
rm -f "$RAMIFY_STATE_DIR/sessions/sl.root"

# ── down leaves shared things alone ──
RAMIFY_ROOT="$WT" "$RAMIFY" down >/dev/null 2>&1
sleep 1
check "down stops the tree's web"               bash -c "! curl -sf -o /dev/null --max-time 2 http://localhost:17201/"
check "down keeps the shared api"               answers 17100
check "down releases the slot"                  test ! -f "$RAMIFY_STATE_DIR/app/slots/1"
check "ping fails once down"                    bash -c "! RAMIFY_ROOT='$WT' '$RAMIFY' ping"

# ── cleanup: a worktree deleted under a running stack ──
git worktree remove --force "$WT2"
"$RAMIFY" cleanup >"$T/cleanup.log" 2>&1
sleep 1
check "cleanup stops the deleted tree's api"    bash -c "! curl -sf -o /dev/null --max-time 2 http://localhost:17002/"
check "cleanup releases its slot"               test ! -f "$RAMIFY_STATE_DIR/app/slots/2"
check "cleanup keeps the shared api"            answers 17100
"$RAMIFY" shared-down >/dev/null 2>&1

# ── prune: a merged branch (GitHub merge commit) ──
git -C "$WT" add -A && git -C "$WT" commit -qm feat
git merge -q --no-ff story/feat -m "Merge pull request #1 from someone/story/feat" 2>/dev/null
git push -q origin main 2>/dev/null
check "prune dry run lists the merged branch"   bash -c "'$RAMIFY' prune | grep -q 'prune  story/feat'"
check "prune dry run deletes nothing"           test -d "$WT"
"$RAMIFY" prune --apply >/dev/null 2>&1
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
printf 'RAMIFY_JIRA_URL=http://127.0.0.1:17350\n' >>.ramify.conf
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

echo
[ "$fails" = 0 ] && echo "smoke: all passed" || echo "smoke: $fails failed"
exit "$fails"
