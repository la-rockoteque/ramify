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

Requirements: bash, git ≥ 2.38 (`merge-tree --write-tree`), curl, lsof, python3, rsync.
Docker only if the repo has shared containers.

## Usage

Set up a repo once. The draft comes from what the repo shows; the `ramify:setup` skill reviews
it with you:

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
| `new <slug>` | Cut a worktree and branch from the remote main, copy gitignored paths, bootstrap, start the stack |
| `label [slug]` | Print `/rename` and `/color` for the session |
| `up` / `down` | Start or stop this worktree's stack |
| `ping` | Exit 0 when every service of this worktree answers |
| `card` / `dash` / `watch [s]` / `status` | Report one worktree, every worktree, on a loop, or the slot table |
| `cleanup` | Release crashed claims, kill orphans, stop stacks of deleted worktrees |
| `prune [--apply]` | Delete merged local branches and their worktrees |
| `delete <worktree> [--yes]` | Force-delete one worktree and its branch |
| `shared-down` | Stop the shared service instances |
| `write-set <file>` | Check the branch's diff against allowed and immutable path globs |

`RAMIFY_ROOT=<path>` points any command at another worktree.

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
| `<svc>_prepare` `_smoke` `_optional` `_timeout` | — | Pre-start step for a private instance, wiring check, start last, readiness timeout |
| `RAMIFY_QA_PLAN` `_RESULTS` `_NOTES` | none | The QA recipe the `ask-for-qa` skill follows |
| `RAMIFY_GATE_BEHIND` | `1` | Refuse a push while the branch is behind main |
| `RAMIFY_MAX_SLOT` / `RAMIFY_SKIP` / `RAMIFY_AUTOSTART` | `10` / none / `1` | Slot count, services to leave out, hook autostart |

### The plugin

The hooks do nothing in a repo without a config.

- **PreToolUse `git push`**: refuses the push while the branch is behind main.
- **PostToolUse `git worktree add` / `ramify new`**: moves the session to the new worktree and starts its stack.
- **PostToolUse `git push`**: tells the agent to hand the branch over for QA.
- **Stop**: starts the stack if it does not answer. The fast path is one curl per service.
- **SessionEnd**: stops this worktree's stack.

Skills: `ramify:setup`, `ramify:worktree`, `ramify:ask-for-qa`, `ramify:qa-dash`.

State lives in `/tmp/ramify/<project>/`, machine-wide, so every session sees the same slot registry.

## Test

```bash
test/smoke.sh                 # new, up, shared/private, down, cleanup, prune, write-set, hooks, setup
ramify write-set --self-test
```

## License

UNLICENSED
