<img src="assets/ramify-512.png" alt="ramify icon: a windswept bonsai before a red sun" width="160" align="right">

# ramify

Run several branches at once, each in its own git worktree with its own QA stack, for AI coding agents and the humans who review their work.

## Background

An agent on one branch and an agent on another cannot share a checkout, and they cannot share
dev-server ports. ramify gives each branch a worktree and each worktree a numbered port slot.
Databases and emulators run once and all worktrees share them. A slow service that a branch
does not change (a backend under a frontend-only branch) also runs once, from the primary
checkout. When the branch is ready, ramify gives the human reviewer the URLs and the checklist.

It ships as one CLI and a Claude Code plugin. The plugin has hooks that start and stop stacks
with the session, and skills that tell the agent how to use them.

## Install

```bash
ln -s ~/dev/Perso/ramify/bin/ramify ~/.local/bin/ramify     # the CLI, for you and the agent
```

In Claude Code:

```
/plugin marketplace add ~/dev/Perso/ramify
/plugin install ramify@ramify
```

A teammate installs from the remote instead:

```
/plugin marketplace add https://github.com/la-rockoteque/ramify.git
/plugin install ramify@ramify
```

The plugin is installed once per machine. Each repo needs only its config, see [Usage](#usage).

Requirements: bash, git ≥ 2.38 (`merge-tree --write-tree`), curl, lsof, python3, rsync.
Docker only if the repo has shared containers. Node.js ≥ 22.12 only to install the app.

## Usage

Set up a repo once. In Claude Code, ask for `/ramify:setup`: it drafts the config, checks it
against the repo, asks what only you know, and proves it with `ramify up`. By hand:

```bash
cd ~/dev/my-app && ramify setup            # writes .ramify.conf (--local: .claude/ramify.conf)
ramify config                              # shows what it resolved
```

Then, per branch:

```bash
ramify new login-form      # worktree + branch from origin/main, copy, bootstrap, stack up
ramify card                # this worktree's URLs and health
ramify dash                # every stack on the machine; `ramify watch` redraws it
ramify down                # stop this worktree's own services
ramify prune --apply       # remove merged branches and their worktrees
```

| Command | What it does |
|---|---|
| `setup [--print\|--force] [--local]` | Draft a config from the repo |
| `config [-q\|get VAR]` | Show the resolved config, or one value |
| `new <slug> [--workflow W [--lane L] [--tag T]…]` | Cut a worktree and branch from the remote main, copy gitignored paths, bootstrap, start the stack. With `--workflow`, also write the work order, see [Workflows](#workflows) |
| `workflow [list\|show <workflow>]` | The workflows of this repo, or the lanes and steps of one |
| `step [done <step> [--approved]\|na <step>\|gate <step>\|--pr] [--note TEXT]` | This branch's work order and next step, record a step, or exit 1 while a step waits on earlier ones |
| `lane <lane>` | Raise this branch's lane. A lane is never lowered |
| `label [slug]` | Print `/rename` and `/color` for the session |
| `up` / `down` | Start or stop this worktree's stack |
| `ping` | Exit 0 when every service of this worktree answers |
| `card` / `dash [--json]` / `watch [s]` / `status` | Report this worktree, every stack of every project (from any directory), the same on a loop, or the slot table |
| `app` | Open the desktop dashboard, see [The app](#the-app) |
| `cleanup` | Release crashed claims, kill orphans and strays, stop stacks of deleted worktrees, close the tickets of merged branches |
| `prune [--all] [--apply]` | Delete merged local branches and their worktrees, detached trees already in main, and close their tickets. `--all` does it in every project on the machine. Then Docker, once: it shows disk use and removes stopped non-compose containers, unused images and build cache. Volumes and compose containers stay |
| `complete <worktree> [--yes]` | Close its ticket, stop its stack, remove the worktree and its local branch |
| `jira login` / `jira check` | Store a Jira API token in the Keychain, or test it |
| `delete <worktree> [--yes]` | Force-delete one worktree and its branch |
| `shared-down` | Stop the shared service instances |
| `statusline [--json]` | A status line segment: the session's worktree and branch |
| `write-set <file>` | Check the branch's diff against allowed and immutable path globs |

`RAMIFY_ROOT=<path>` points any command at another worktree.

On a terminal, `card`, `dash` and `watch` draw each project's tiles in a colour of its own, taken
from its name. A state is green when ready or up, yellow when starting and red when down.
The card of the main branch is filled with the project colour. Each shared service has a colour of
its own: the shared instance and every tile that uses it show in that colour.
`NO_COLOR` turns colour off. Output through a pipe and `--json` stay plain.

`dash`, `watch` and `dash --json` (`"mb"`) show the resident memory of each container and each
service. A service counts its whole process group, build servers left out. A shared instance shows
on the shared line only, not on each tile that uses it. Container memory comes from
`docker stats`, which adds about 2s to each draw.

### The config

`.ramify.conf` is sourced bash. Single-quote the commands so `$PORT` expands when the service
starts. A minimal one:

```bash
RAMIFY_BRANCH_PREFIX=feat/
RAMIFY_BOOTSTRAP='npm install'
RAMIFY_READY=node_modules/.package-lock.json
RAMIFY_SERVICES="web"
web_port=5173                # slot N runs on 5173+N
web_health=/
web_cmd='npm run dev -- --port $PORT'
```

[examples/moship.conf](examples/moship.conf) is a full one: shared containers, an ASP.NET API
that is shared until the branch touches the backend, migrations for a private API only, a Vite
SPA that proxies to whichever API it got, and Storybook.

| Variable | Default | Meaning |
|---|---|---|
| `RAMIFY_MAIN` / `RAMIFY_REMOTE` | `main` / `origin` | What branches are cut from and compared to |
| `RAMIFY_BRANCH_PREFIX` | none | `new <slug>` creates `<prefix><slug>` |
| `RAMIFY_WORKTREES` | `../<repo>-wt` | Where `new` puts worktrees, relative to the primary checkout |
| `RAMIFY_COPY` | none | Gitignored paths copied into a new worktree |
| `RAMIFY_BOOTSTRAP` / `RAMIFY_READY` | none | Install command, and the file that exists once it is done |
| `RAMIFY_COMPOSE_FILE` / `RAMIFY_CONTAINERS` | none | Shared containers, started under a fixed compose project and never stopped |
| `RAMIFY_SERVICES` | none | Services in start order |
| `<svc>_port` `_dir` `_health` `_cmd` | — | Base port, directory, readiness path, command |
| `<svc>_shared_port` `_shared_cmd` `_shared_when` | — | Run once from the primary checkout while every changed path matches `_shared_when` |
| `<svc>_when` | — | Start the service only when a changed path matches this regex |
| `<svc>_prepare` `_smoke` `_optional` `_timeout` | — | Pre-start step for a private instance, wiring check, start last, readiness timeout |
| `RAMIFY_QA_PLAN` `_RESULTS` `_NOTES` | none | The QA recipe the `ask-for-qa` skill follows |
| `RAMIFY_JIRA_URL` | none | `https://<site>.atlassian.net`: turns on ticket closing, see [Tickets](#tickets) |
| `RAMIFY_TICKET_PATTERN` / `RAMIFY_JIRA_DONE` | `[A-Za-z][A-Za-z0-9]*-[0-9]+` / none | The key in a branch name; the done transition to pick when there are several |
| `RAMIFY_GATE_BEHIND` | `1` | Refuse a push while the branch is behind main |
| `RAMIFY_GATE_WORKFLOW` | `1` | `0` lets `gh pr create` through before the work order is ready |
| `RAMIFY_WORKFLOWS` | `1` | `0` turns workflows off for the repo, see [Workflows](#workflows) |
| `RAMIFY_MAX_SLOT` / `RAMIFY_SKIP` / `RAMIFY_AUTOSTART` | `5` / none / `1` | Slot count (stacks running at once), services to leave out, hook autostart |

### Processes

Each service leads its own process group. Its environment carries
`RAMIFY_TAG=<project>@<hash>/<worktree>/<service>`, and every child inherits it. The hash comes
from the state directory, so two repos with the same name stay apart. A shared instance has
`@shared` as its worktree. ramify signals a group only when a member carries a tag of the
project. A stack started before tags is also stopped when its pid file names the group that
listens on its port. ramify leaves any other process alone and says so: a reboot keeps `/tmp`,
and the system gives old pids and ports to new programs.

- `down` stops every tagged group of the worktree, also the groups that no pid file names.
- A **stray** is a tagged group that no pid file names. A worktree deleted from the Finder
  leaves strays. So does an `up` that overwrote the pid file of an older one. `dash` and the
  app list strays, and `cleanup` stops them.
- Only one `up` runs per worktree at a time. A second one sees the lock and exits. The Stop
  hook fires after every turn, and without the lock it starts a second stack beside a slow API.
- **Build servers** (reusable MSBuild nodes, the Roslyn `VBCSCompiler`, the MSBuild and Razor
  servers) serve every checkout on the machine and outlive the build. One that a service's build
  starts joins the service's group and inherits its tag. ramify still never signals it: it
  stops a group member by member and leaves build servers out. They exit on their own when idle,
  Roslyn after 10 minutes and MSBuild nodes after 15. `dash` and the app show their count and
  memory. `dotnet build-server shutdown` stops them.

### Tickets

With `RAMIFY_JIRA_URL` set, ramify reads a Jira key from each branch name as a whole word:
`story/tm-127-tracker-pagination` is TM-127, and `story/backend-under-3min` has no key. Then:

- `prune --apply` closes the ticket of each branch it removes. The dry run says what it would close.
- `cleanup` closes the open tickets of merged branches that are still here. It removes nothing.
- `complete <worktree>` closes the ticket, then removes the worktree and its local branch. It
  refuses uncommitted or unpushed work. If Jira refuses, the worktree stays.

"Closed" means the status category done, so a ticket already in any done status is left alone.
Run `ramify jira login` once per Jira site. It stores your email and an
[API token](https://id.atlassian.com/manage-profile/security/api-tokens) in the macOS Keychain,
not in the repo config. On Linux, export `RAMIFY_JIRA_EMAIL` and `RAMIFY_JIRA_TOKEN` instead.

### Workflows

A workflow is the path a piece of work takes, from the spec to the merge. ramify ships four in
[workflows.yml](workflows.yml): `feature`, `bug`, `tweak` and `spike`. Each workflow has one or
more lanes. A lane is the ordered list of steps that work goes through. `feature` has `light`
and `standard`, and `bug` has `hotfix` and `standard`.

```bash
ramify new login-form --workflow feature --lane light --tag auth   # auth puts it on standard
ramify step                          # the work order, ✓ recorded, → next, with its skill
ramify step done spec --approved     # a human gate: only after the user's explicit OK
ramify step na qa-plan --note "no UI"
ramify lane standard                 # raise the lane; lowering is refused
```

- `new --workflow` takes the branch name from the workflow (`feat/login-form`), and writes the
  work order to `.ramify/work/<slug>/order.md`. The work order is YAML frontmatter
  (`workflow`, `lane`, `branch`, `tags`, `created`, `steps`), then free prose. Commit it with
  the branch. The step outputs go in the same folder.
- The `branch` in the frontmatter ties the work order to its worktree. A slug whose work order
  is already on main is refused: pick another one.
- Steps are recorded in lane order. A step that does not apply is recorded `na`. `finale` is not
  recorded: it removes the worktree that holds the work order.
- A step with `gate: human` is recorded only with `--approved`, after the user's explicit OK.
- The step marked `opens_pr: true` (`pr` in the defaults) waits on every step before it. The
  plugin refuses `gh pr create` and `gh pr ready` until they are recorded, and names the next
  one with its skill. `gh pr create --draft` (or `-d`) is not gated. A broken work order does not
  block the PR either: `ramify step` says what is wrong. The command is parsed as shell words, so
  `gh -R owner/repo pr create` is caught, but an alias or `$(which gh)` is not. A lane without such a step opens
  PRs freely. `ramify step gate --pr` runs the same check.
- `min_lane` maps a tag to the lightest lane allowed: `--tag auth` puts a feature on `standard`.
  A lane goes up, never down.

The reports show the progress of a branch with a work order. `card` and `dash` add a line
`feature/light 3/6 → review` to its tile, and the status line appends the same. The app draws
one segment per step of the lane: green when recorded, grey when n/a, yellow for the next one.
`dash --json` gives each worktree a `workflow` object (`workflow`, `lane`, `order`, `done`,
`total`, `next` {`step`, `skill`, `gate`}, `steps` [{`step`, `state`}]), or `null` without one.
It is `{"error"}` when the definitions do not load, or when the work order names a workflow or
lane they do not define. A work order without frontmatter, or without `workflow`, `lane` and
`branch`, belongs to no branch and is not reported.

A repo replaces the defaults with its own `.ramify/workflows.yml` in the same shape, or turns
workflows off with a file that holds `workflows: off`. `RAMIFY_WORKFLOWS=0` in the config does
the same. A repo with workflows off keeps its own process, and ramify does not touch it. The
YAML is parsed with a vendored copy of PyYAML 6.0.2 (pure python, MIT, `libexec/vendor/yaml`).

### The status line

A session that `ramify new` moved to a worktree still reports the directory it started in, so a
status line shows that checkout's branch. `ramify statusline` reads the status line JSON and
follows the session to its worktree. It prints `𖣂 tm-127-pagination ⎇ story/tm-127-pagination · 2/3 up`,
or `𖣂 main` in the primary checkout. The count says how many of the worktree's services
answer; `stack down` means none do. It prints nothing where ramify is not set up.

```json
"statusLine": { "type": "command", "command": "ramify statusline" }
```

`ramify` must be on the PATH (see [Install](#install)): the plugin's `bin/` is on the Bash
tool's PATH only. A status line script of your own can call `ramify statusline --json` for
`root`, `worktree`, `branch`, `primary`, `icon`, `slot`, `ticket`, `stack` (`up`, `partial`,
`down` or `stopped`) and `workflow` (as in `dash --json`), and run its git segment from `root`.
`RAMIFY_STATUSLINE_ICON` replaces the tree.

### The app

`app/` is an Electron window on the same machine-wide dashboard. It shows every project and
every worktree, running or not, one tab per repo. From it you start and stop stacks, cut a new worktree, complete or
delete one, prune merged branches, run cleanup and stop the shared instances. It reads
`ramify dash --json` every 3 seconds, and each button runs a `ramify` command. The output goes
to the log panel. The CLI stays the source of truth.

A red **!** marks a failure: a service that did not come up, a `prepare` or bootstrap that
failed, Docker down, no free slot, or a service that stopped answering. Click it to read the
log. `up` records each failure in `/tmp/ramify/<project>/<worktree>.errors`. The next `up`
starts that file afresh, and `down` clears it.

In Claude Code, ask for `/ramify:app`. In a terminal, run `ramify app`. The first run installs
Electron into `~/.cache/ramify/electron` (Node.js ≥ 22.12 and npm needed, once per Electron
version). Plugin updates reuse it. The window runs detached from the shell that opened it.

### The plugin

The hooks do nothing in a repo without a config. The workflow hooks need only a `.ramify` folder.

- **PreToolUse `git push`**: refuses the push while the branch is behind main.
- **PreToolUse `gh pr create` / `gh pr ready`**: refuses while the work order has steps missing before its PR step. Drafts pass.
- **SessionStart** (startup, resume, clear, compact): gives the agent the branch's work order and its next step.
- **PostToolUse `git worktree add` / `ramify new`**: moves the session to the new worktree and starts its stack. With `--workflow`, it also gives the agent the work order.
- **PostToolUse `git push`**: tells the agent to hand the branch over for QA.
- **Stop**: starts the stack if it does not answer. The fast path is one curl per service.
- **SessionEnd**: stops this worktree's stack.

Skills: `ramify:setup`, `ramify:worktree`, `ramify:ask-for-qa`, `ramify:qa-dash`, `ramify:app`.
Workflow skills: `ramify:workflow` (the contract every step follows), then one per step of the
feature lanes: `ramify:spec`, `ramify:qa-plan`, `ramify:build`, `ramify:audit`, `ramify:qa`,
`ramify:review`, `ramify:pr`, `ramify:finale`. Every step runs the prose it writes through the
[ocre-jelly](https://git.nexapptech.com/vbernier/ocre-jelly) skill. Without it, the step follows
the same rules by hand and says so.

State lives in `/tmp/ramify/<project>/`, machine-wide, so every session sees the same slot registry.
The list of configured repos lives in `~/.claude/ramify/projects` (`$CLAUDE_CONFIG_DIR` if set),
one primary checkout per line. Any `ramify` command in a configured repo adds it. A reboot keeps
it, so the app lists a repo whose stacks are all down. A moved repo, or one without its config
any more, drops out of the list; delete its line to forget it for good.

## Test

```bash
test/smoke.sh                 # new, up, shared/private, dash --json, memory, strays, up lock, down, cleanup, prune, write-set, workflows, hooks, setup
ramify write-set --self-test
```

## License

UNLICENSED
