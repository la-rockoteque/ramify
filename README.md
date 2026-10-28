<img src="assets/ramify-512.png" alt="ramify icon: a windswept bonsai before a red sun" width="160" align="right">

# ramify

Run several branches at once, each in its own git worktree with its own QA stack, for AI coding agents and the humans who review their work.

## Background

An agent on one branch and an agent on another cannot share a checkout, and they cannot share
dev-server ports. ramify gives each branch a worktree and each worktree a numbered port slot.
Databases and emulators run once and all worktrees share them. A slow service that a branch
does not change (a backend under a frontend-only branch) also runs once, from the primary
checkout. When the branch is ready, ramify gives the human reviewer the URLs and the checklist.

It ships as one CLI with plugins for Claude Code and Codex. Both agents share the same
work orders, configuration, approvals, and QA stack state. The plugins supply lifecycle hooks
and skills that tell the agent how to use the CLI.

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

### Codex

Ramify includes a Codex manifest in `.codex-plugin/plugin.json` and a local marketplace in
`.agents/plugins/marketplace.json`. Add the GitHub marketplace:

```bash
codex plugin marketplace add la-rockoteque/ramify
```

For a local checkout, add that directory instead:

```bash
codex plugin marketplace add .
```

Restart the desktop app, open the Plugins Directory, choose **Ramify**, and install the plugin.
Review and trust its hook definitions when prompted. Installing a plugin alone does not trust its hooks.
See the official [plugin packaging guide](https://developers.openai.com/plugins/build/plugins) and
[hook documentation](https://learn.chatgpt.com/docs/hooks) for supported clients and trust controls.

The CLI installation above serves both agents. No separate configuration or state migration is needed.
The existing `.claude/ramify.conf` and `~/.claude/ramify/` paths remain shared for compatibility.

### Continue Claude work in Codex

1. Stop Claude's edits and open Codex in the same worktree. `git worktree list` lists the paths.
2. Ask: “Use Ramify to resume the work Claude left in this worktree.” The `ramify:resume` skill reads the existing state.
3. Codex reads the work order, earlier step outputs, pending decisions, and unfinished Git changes.
4. It continues the next incomplete step. A pending human approval still waits for your decision.

Uncommitted and untracked edits stay in place. A handoff does not need a new branch, work order,
or chat transcript. Use one active editor in that worktree at a time. A partly completed step
resumes from its files; only completed steps belong in the workflow record.

If hooks are disabled, untrusted, or unavailable in your client, invoke `ramify:resume` explicitly.
From that worktree, these commands inspect the shared state:

```bash
ramify step
ramify step asked --json
ramify card
```

Use `ramify up` when you need a stopped QA stack. Use `ramify down` when you finish with it.
After Claude closes its session, its SessionEnd hook may have stopped the stack. Codex can restart it
in the same worktree. The workflow record and pending decisions survive that shutdown.

Wait commands work with Claude's background Bash or Codex's process session tools.
See the [runtime instructions](skills/workflow/SKILL.md#cli-and-runtime-tools).


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
| `step ask <step> [--file F]…` / `step wait <step>` | Put a human gate up in the approvals window, or block until the human decides. See [Approvals](#approvals) |
| `approve <step> [--note T]` / `reject <step> --note T` | The human's decision on a gate the agent asked about |
| `gates [--json]` / `approvals` | Every gate on the machine that waits on a decision, or open the approvals window |
| `scratch [--title T] [FILE \| -]` / `scratch --list\|--clear` | Hand the human a text to copy. See [Scratch](#scratch) |
| `run ask <script>` / `run wait <id>` / `runs [--json]` | Hand the human a script to run or block, or wait for the decision. See [Run](#run) |
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
| `ticket start` / `ticket sync` / `ticket watch` | Assign the story and start work, synchronize once, or resume automatic synchronization |
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
| `RAMIFY_JIRA_PROGRESS` / `RAMIFY_JIRA_REVIEW` | `In Progress` / none | Exact Jira status, transition name, or transition ID for development and review |
| `RAMIFY_JIRA_SYNC` / `RAMIFY_TICKET_INTERVAL` | `1` / `60` | Automatic story synchronization; polling interval in seconds (minimum 5) |
| `RAMIFY_GIT_HOST` / `RAMIFY_GIT_API_URL` | detected / detected | Host adapter (`github`, `gitlab`, `bitbucket`) and API base URL; set the adapter for self-hosted GitHub or GitLab |
| `RAMIFY_GATE_BEHIND` | `1` | Refuse a push while the branch is behind main |
| `RAMIFY_GATE_WORKFLOW` | `1` | `0` lets `gh pr create` through before the work order is ready |
| `RAMIFY_WORKFLOWS` | `1` | `0` turns workflows off for the repo, see [Workflows](#workflows) |
| `RAMIFY_WORKFLOW_SOURCE` / `RAMIFY_WORKFLOW_DOCS` | `auto` / none | Setup selects repository guides or Ramify definitions; guide paths are a JSON array relative to the checkout |
| `RAMIFY_MAX_SLOT` / `RAMIFY_SKIP` / `RAMIFY_AUTOSTART` | `5` / none / `1` | Slot count (stacks running at once), services to leave out, hook autostart |
| `RAMIFY_AUTO_ALLOW` | `1` | `0` turns off the hook that runs plain `ramify` commands without a prompt |
| `RAMIFY_SESSION_NAMES` | `trees` | The theme of session names: `trees`, `birds`, a list of words, or `off`. See [Session names](#session-names) |

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

On macOS, each desktop window has a named app bundle: Ramify, Ramify Approvals, Ramify Scratch, or Ramify Run.
Reopening one of these bundles opens Ramify without a command-line argument.
The launcher waits for the actual window to acknowledge readiness before reporting success.
Startup errors remain visible and logs live under `~/.cache/ramify/electron/launch/` (or `RAMIFY_APP_CACHE/launch/`).
Opening or refreshing Run displays scripts without executing them. Execution requires a deliberate click on Run.

### Tickets

With `RAMIFY_JIRA_URL` set, ramify reads a Jira key from each branch name as a whole word:
`story/tm-127-tracker-pagination` is TM-127, and `story/backend-under-3min` has no key. Then:

- `new` assigns the story to your Jira account and moves it to In Progress in the background.
- A local watcher moves the story to In Review when a PR or MR opens, including drafts.
- A confirmed merge moves the story to Done, including squash merges. A declined or closed unmerged request does not finish the story.
- `prune --apply` closes the ticket of each branch it removes. The dry run says what it would close.
- `cleanup` closes the open tickets of merged branches that are still here. It removes nothing.
- `complete <worktree>` closes the ticket, then removes the worktree and its local branch. It
  refuses uncommitted or unpushed work. If Jira refuses, the worktree stays.

"Closed" means the status category done, so a ticket already in any done status is left alone.
Run `ramify jira login` once per Jira site. It stores your email and an
[API token](https://id.atlassian.com/manage-profile/security/api-tokens) in the macOS Keychain,
not in the repo config. On Linux, export `RAMIFY_JIRA_EMAIL` and `RAMIFY_JIRA_TOKEN` instead.

Configure the project's statuses in `.ramify.conf`:

```bash
RAMIFY_JIRA_URL=https://example.atlassian.net
RAMIFY_JIRA_PROGRESS='In Progress'
RAMIFY_JIRA_REVIEW='In Review'
RAMIFY_JIRA_DONE='Done'
RAMIFY_TICKET_INTERVAL=60
```

Review needs an explicit mapping because Jira has no review category. Each mapping accepts an exact status, transition name, or transition ID.
If a mapping matches no available transition or several transitions, synchronization reports the error and retries.
Set `RAMIFY_JIRA_DONE` when the workflow offers several done transitions.

The watcher supports GitHub, GitLab, and Bitbucket Cloud. It reads the repository from `RAMIFY_REMOTE` and matches the source branch and `RAMIFY_MAIN` target.
Sign in with `gh auth login` or `glab auth login`, or export `RAMIFY_GIT_TOKEN` for the selected host.
GitHub also accepts `GH_TOKEN`; GitLab accepts `GITLAB_TOKEN`. The host token needs permission to read the repository's PRs or MRs.
For Bitbucket Cloud, export `RAMIFY_BITBUCKET_TOKEN` with repository and pull request read access.
For a Bitbucket API token, also export `RAMIFY_BITBUCKET_EMAIL`; repository access tokens use Bearer authentication without that email.
Keep tokens in your environment or credential manager, outside committed configuration.

`ramify ticket start` assigns an existing branch's story and starts the watcher.
`ramify ticket sync` synchronizes once and retries a failed start.
`ramify ticket watch` resumes the watcher without assigning an existing story.
Session startup also resumes it. Only one watcher runs per worktree; it exits when the worktree disappears or changes branch.
Browser actions appear at the next poll while the machine and watcher run. After downtime, use `ticket sync` or resume a session.
Errors leave local work intact. Logs live in `<state-dir>/tickets/`; `ramify config get STATE_DIR` shows the state directory.
Set `RAMIFY_JIRA_SYNC=0` to disable lifecycle synchronization and retain the existing cleanup and completion commands.
Already done stories stay done, and repeated syncs leave matching assignments and statuses unchanged.

### Workflows

`ramify setup` detects an existing development workflow before choosing the defaults.
It recognizes named delivery pipelines such as Moship's `docs/{feature,bug-fix,tweak,spike,release}-pipeline.md`, conventional workflow guides, and guides linked from repository instructions.
Product workflows such as equipment fulfillment do not select a development workflow.
The draft shows the source paths and selection reason; `setup --print` writes nothing.

For a documented repository workflow, the configuration looks like:

```bash
RAMIFY_WORKFLOW_SOURCE=repo
RAMIFY_WORKFLOW_DOCS='["docs/feature-pipeline.md", "docs/bug-fix-pipeline.md"]'
```

`ramify workflow`, `ramify step`, and session startup point to those guides and native work records.
Use the project's skills, steps, gates, and artifact locations. Ramify creates no duplicate work order and adds no default PR checklist.
The repository's existing hooks enforce its own workflow. Worktrees, Git safety, QA stacks, and Jira integration remain available.
Guide paths resolve in the current checkout; setup from a worktree inspects that checkout's workflow documents.

An explicit `.ramify/workflows.yml` takes precedence. An existing Ramify work order continues with its original record.
Set `RAMIFY_WORKFLOW_SOURCE=ramify` to choose Ramify's definitions explicitly.
If no development workflow is detected, setup keeps the defaults below.
If several standalone guides compete, select the authoritative paths explicitly; a missing selected guide reports an error instead of reverting silently.

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

### Repository-native records and automatic gates

A repository can keep its own pipeline and story files while Ramify presents its human gates.
Configure the adapter in ignored `.ramify/workflows.yml`; do not create a second work order.
For a story under `docs/stories/{todo,active,audit,completed}/<slug>/story.md`:

```yaml
work_dir: docs/stories/*
record:
  filename: story.md
  workflow_field: kind
  slug_field: story
  date_field: "on"
  note_field: result
auto_ask: true
workflows:
  feature:
    branch: story/{slug}
    lanes:
      standard: [qa-plan, implement-build, review, create-pr]
steps:
  qa-plan: {gate: human, files: [story.md, qa-plan.md]}
  implement-build: {}
  review: {gate: human, files: [review.md, qa-results.md]}
  create-pr: {opens_pr: true}
```

Match the steps and lanes to the repository's actual pipeline. This example shows only four steps.
Ramify derives the branch from the configured template and story slug when the record has no explicit branch.
It preserves native metadata and writes step dates and notes under the configured names.
Moving the story between stage folders keeps its gate. Two records for one branch are an error.

With `auto_ask: true`, hooks request the next human gate once its configured files exist.
They open the approvals window after tool use, at session start and before the agent stops.
`ramify step ensure` runs the same check by hand and leaves an unchanged pending request intact.
A rejection stays rejected until its documents change or the agent explicitly requests another decision.
An approval binds to the displayed document contents. Changed documents need a refreshed request.

The edit guard blocks implementation while the next step requires human approval.
Planning files remain editable. Configure `planning_paths` to replace its default patterns:
`docs/**`, `.ramify/**` and `.claude/**`.
The guard covers file edits, patches and shell tools. Unknown shell scripts wait for approval.
Shell read commands and planning-only Git staging or commits remain available.
Both Claude Code and Codex register the guard; existing shell hooks also apply it.
`ramify step gate --human` reports a pending gate for other integrations.

### Approvals

A human gate (`spec`, `qa-plan`, `review`…) waits on a person. The agent puts it up, the person
decides in a small window, and the agent goes on by itself.

```bash
ramify step ask spec          # agent: record the request, pop the approvals window open
ramify step wait spec         # agent, in the background: exits on the decision
ramify approve spec           # human, or the Approve button: records the step --approved
ramify reject spec --note "add the error state"   # human, or the Reject button
```

- `step ask` shows `.ramify/work/<slug>/<step>.md` and the work order. `--file` names other
  files instead. A file must be inside the worktree.
- The request is `ask:` in the work order's frontmatter: `step`, `asked`, `files`. A rejection
  adds `decision`, `decided` and `note`. An approval records the step and removes `ask:`.
- `step wait` prints `approved[: note]` or `rejected: note` and exits 0. With `--timeout S` it
  exits 1 when nobody decided. Claude Code uses `run_in_background`; Codex keeps the returned
  process session and reads its completion output. A timeout leaves the decision pending.
- An OK in the chat still works: `ramify step done <step> --approved` also closes the request.
- The plugin refuses `ramify approve` and `ramify reject` from the agent.

The approvals window lists every open request on the machine, one tab each. It renders the
markdown, and has a note field, **Reject** (a note is required) and **Approve** (⌘↩). It runs on
the same Electron as [the app](#the-app). A second `step ask` brings the open window to the front.
The window closes once the last request is answered. `RAMIFY_POPUP=0` keeps it shut, and
`ramify gates` lists the requests in a terminal.

`card`, `dash` and the status line show `⏳ spec waits on you` while a request is open, and
`dash --json` gives it as the `asked` step of the `workflow` object.

### Scratch

An agent often writes a text for the human to paste elsewhere: a Slack message, a command, a
ticket body. `ramify scratch` puts it in a small window with a **Copy** button.

```bash
ramify scratch --title "Slack: release note" note.md   # from a file
gh pr view 12 --json body -q .body | ramify scratch    # from stdin
ramify scratch --list                                   # what waits in the window
```

- The window shows the text as it will paste, one tab per item. The newest item opens in front.
- **Copy** (⌘C) puts the whole item on the clipboard. ⌘C with a selection copies the selection.
- **Done** deletes the item. ⌘↩ copies, then deletes. The window closes after the last one.
- Without `--title`, the title is the file name, or the first line of the text.
- Items live in `$RAMIFY_STATE_DIR/scratch` (`/tmp/ramify/scratch`), one `<id>.txt` and
  `<id>.title` each. `RAMIFY_SCRATCH_DIR` moves them. `RAMIFY_POPUP=0` keeps the window shut.

### Run

Some commands an agent may not run itself: a write to production, a data import, a change to a
shared account. The agent writes them as a script, and the human runs or blocks it in a small
window. The output goes back to the agent.

```bash
ramify run ask reseed.sh --cwd . --note "Reseed prod requests"   # agent: freeze the script, pop the run window open
ramify run wait 20261016-104206-88917   # agent, in the background: exits once the human ran or blocked it
ramify run go 20261016-104206-88917     # human, or the Run button
ramify run block 20261016-104206-88917 --note "not before the snapshot"   # human, or the Block button
ramify runs                             # every run, newest first
```

- `run ask` copies the script into its run folder, read-only, and records its sha256. Run checks
  the copy against that hash, so what the human read is what runs. A changed copy is blocked and
  nothing runs.
- The script runs as the human, with bash, in the `--cwd` directory (the agent's own by default).
  Its input is `/dev/null`: a script that asks a question gets no answer. It inherits the
  environment of the window, so an AWS SSO profile or a `gh` login works.
- `run wait` prints `ran: exit <n>` and the last 40 lines of output, or `blocked: <reason>`, and
  exits 0. With `--timeout S` it exits 1 when nobody decided. A run runs once.
- The window lists the runs that wait, then the last 20 finished ones. It shows the note, the
  directory, the hash and the script with line numbers. Run has no keyboard shortcut. A started
  run outlives the window, and the window streams its output.
- A run is a folder in `~/.claude/ramify/runs/<id>`: `script`, `meta.json` and `output.log`.
  `RAMIFY_RUNS_DIR` moves them. `RAMIFY_POPUP=0` keeps the window shut, and a bare `ramify run`
  opens it.
- The plugin refuses `ramify run go` and `ramify run block` from the agent.

`card`, `dash` and the status line show `▶ run <id> waits on you` while a run asked from that
worktree waits.

### Session names

Each agent session gets a short name, such as `birch`, so you can tell sessions apart. The
`session-start` hook picks a word from the theme at random. It skips the names that live
sessions on the machine hold. When every word is held, it adds a number: `birch-2`, up to
`-9`. Past that, the session gets no name.

- `RAMIFY_SESSION_NAMES` sets the theme. `trees` is the default. `birds` is the other built-in
  theme. Several words separated by spaces are a theme of their own. `off` turns names off.
  A single word that names no theme is a theme of one word: `ramify config` warns about it.
- A built-in theme is `assets/names/<theme>.txt`, one word per line. To add a theme, add a file.
- The session title is `<slug> · <name>` in a linked worktree and `<name>` in the primary
  checkout. A title you set wins.
- The name is in `${RAMIFY_STATE_DIR:-/tmp/ramify}/sessions/<session_id>.name`, one line. A
  status line reads it with the `session_id` of its JSON input.
- When the session ends, by `/exit` or any other way, the name is released. A resumed or
  cleared session takes the name back from its title, unless another session holds it.

### The status line

A session that `ramify new` moved to a worktree still reports the directory it started in, so a
status line shows that checkout's branch. `ramify statusline` reads the status line JSON and
follows the session to its worktree. It prints `𖣂 tm-127-pagination ⎇ story/tm-127-pagination · 2/3 up`,
or `𖣂 main` in the primary checkout. A named session starts with its name: `𖣂 birch · main`. The count says how many of the worktree's services
answer; `stack down` means none do. It prints nothing where ramify is not set up.

```json
"statusLine": { "type": "command", "command": "ramify statusline" }
```

`ramify` must be on the PATH (see [Install](#install)): the plugin's `bin/` is on the Bash
tool's PATH only. A status line script of your own can call `ramify statusline --json` for
`root`, `worktree`, `branch`, `name` (the [session name](#session-names), or null), `primary`, `icon`, `slot`, `ticket`, `stack` (`up`, `partial`,
`down` or `stopped`), `workflow` (as in `dash --json`) and `ports` (each running service and its
port, `{"web": 5174}`), `run` (`▶ run <id> waits on you`, or null), and run its git segment from `root`.
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

### The plugins

Claude loads hooks from `.claude-plugin/plugin.json`. Codex loads `hooks/codex.json` through
its Codex manifest. Codex shell hooks match `Bash`, including `exec_command`; the supported
`SessionStart`, `PreToolUse`, `PostToolUse`, `Stop`, and `SessionEnd` events share the dispatcher.
Codex SessionEnd can occur after an idle delay; switching away from a conversation does not
immediately stop its stack. Use `ramify down` when you need to stop it explicitly.

The hooks do nothing in a repo without a config. The workflow hooks need only a `.ramify` folder.
The allow hook works in every repo, since a ramify command acts the same anywhere.

- **PreToolUse `git push`**: refuses the push while the branch is behind main.
- **PreToolUse `gh pr create` / `gh pr ready`**: refuses while the work order has steps missing before its PR step. Drafts pass.
- **SessionStart** (startup, resume, clear, compact): gives the agent the branch's work order and its next step. It also picks the [session name](#session-names). In Claude Code, startup, resume and fork set the session title to `<slug> · <name>` in a linked worktree and `<name>` in the primary checkout, unless the session already has a title. Codex receives the work order without a session title.
- **PostToolUse `git worktree add` / `ramify new`**: moves the session to the new worktree and starts its stack. With `--workflow`, it also gives the agent the work order.
- **PostToolUse `git push`**: tells the agent to hand the branch over for QA.
- **Stop**: starts the stack if it does not answer. The fast path is one curl per service.
- **PreToolUse `ramify approve` / `ramify reject` / `ramify run go` / `ramify run block`**: refuses them. The decision on a gate or a run belongs to the human.
- **PreToolUse Bash, allow**: lets a plain `ramify` command run without a permission prompt. See [Commands that run without a prompt](#commands-that-run-without-a-prompt).
- **SessionEnd**: stops this worktree's stack.

#### Commands that run without a prompt

In Claude Code, the allow hook answers `allow` when every part of the Bash command is one of these:

- `ramify` with an allowed subcommand, behind `RAMIFY_ROOT=…` only
- `cd <dir>`, with an absolute path
- `head`, `tail`, `grep` or `wc` after a pipe, with flags and counts only, and one pattern for `grep`.
  A file argument, `-f`, or a recursive `grep` gets no answer.

| Allowed | Still prompted |
|---|---|
| `config`, `workflow`, `gates`, `runs`, `label`, `card`, `dash`, `status`, `ping`, `statusline`, `write-set`, `help`, any `--help` | `step done … --approved` |
| `step`, `lane` | `approve`, `reject`, `run go`, `run block` (refused anyway) |
| `run ask`, `run wait`, `scratch`, `approvals`, `app` | `delete`, `complete`, `prune --apply`, `cleanup`, `shared-down` |
| `new`, `up`, `down`, `prune` without `--apply` | `setup`, `jira`, `ticket`, `menu`, `watch` |

The `ramify` must be the one on `PATH` or the plugin's own `bin/ramify`. The agent can edit any
other copy. ramify sources the config of the tree it runs in, so the session's directory, each
`cd` target and a `RAMIFY_ROOT` value must be inside a worktree of the repo. A command with any
of `$`, backslash, backtick, `*`, `?`, `[`, `]`, `{`, `}`, `~`, `!` or `#` gets no answer, even in
quotes: bash expands what the hook cannot see. The only redirections accepted are the whole
tokens `2>&1`, `>/dev/null` and `2>/dev/null`. A deny from
another hook still wins. `RAMIFY_AUTO_ALLOW=0` in `.ramify.conf` turns the hook off for that repo.

The hook trusts the repo's config. Every `ramify` command sources `.ramify.conf` as bash, so an
agent that can edit that file can run code through an allowed command. Allowing `npm test` trusts
`package.json` the same way. Turn the hook off where that trust does not hold.

Skills: `ramify:resume`, `ramify:setup`, `ramify:worktree`, `ramify:ask-for-qa`, `ramify:qa-dash`, `ramify:app`, `ramify:scratch`, `ramify:run`.
Workflow skills: `ramify:workflow` (the contract every step follows), then one per step of the
feature lanes: `ramify:spec`, `ramify:qa-plan`, `ramify:build`, `ramify:audit`, `ramify:qa`,
`ramify:review`, `ramify:pr`, `ramify:finale`. Every step runs the prose it writes through the
[ocre-jelly](https://git.nexapptech.com/vbernier/ocre-jelly) skill. Without it, the step follows
the same rules by hand and says so.

State lives in `/tmp/ramify/<project>/`, machine-wide, so every session sees the same slot registry.
The list of configured repos lives in `~/.claude/ramify/projects` (`$CLAUDE_CONFIG_DIR` if set),
one primary checkout per line. Commands in a configured repo add it, except workflow inspection and waits. A reboot keeps
it, so the app lists a repo whose stacks are all down. A moved repo, or one without its config
any more, drops out of the list; delete its line to forget it for good.

## Test

```bash
test/smoke.sh                 # new, up, shared/private, dash --json, memory, strays, up lock, down, cleanup, prune, write-set, workflows, hooks, setup
ramify write-set --self-test
```

## License

UNLICENSED
