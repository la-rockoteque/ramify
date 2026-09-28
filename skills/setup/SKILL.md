---
name: setup
description: Configure a repo for ramify — its branching conventions, where worktrees live, what a new worktree must copy and bootstrap, the QA recipe, and the services a QA stack runs. Use when a repo has no .ramify.conf, when asked to set up ramify, worktrees or QA stacks for a project, or when its conventions changed.
argument-hint: "[--local]"
---

# Set up a repo

A config is one sourced bash file: `.ramify.conf` (committed, for the team) or
`.claude/ramify.conf` (personal, usually gitignored). The CLI drafts it; you make it true.

## 1. Draft

```bash
RAMIFY_ROOT=<primary checkout> ramify setup --print
```

The draft reads what the repo already shows: the remote's default branch, the branch prefix most
branches share, where linked worktrees already live, gitignored paths that exist (`.claude`,
`.env*`), compose containers, Node dev servers and Storybook, and ASP.NET Core Web projects. It
is a guess from the file tree. Nothing in it is verified yet.

A repo with nothing to serve (a library, a Claude plugin) is set up for branching only: leave
`RAMIFY_SERVICES` empty. `new`, `prune`, `delete` and the `behind` gate work; the QA hooks and
the dashboard skip it.

## 2. Check it against the repo

Read what the draft cannot: `README`, `CLAUDE.md` / `AGENTS.md`, `CONTRIBUTING`, the
`package.json` scripts, `launchSettings.json`, `Procfile`, `Makefile`, `.env.example`, and any
pipeline doc that says how branches and QA work. For each value, confirm or correct:

- **Branching** — `RAMIFY_MAIN`, `RAMIFY_BRANCH_PREFIX`, `RAMIFY_WORKTREES`. A documented
  convention wins over what the branch list suggests.
- **Bootstrap** — `RAMIFY_COPY` (gitignored paths a fresh tree needs), `RAMIFY_BOOTSTRAP` (the
  install), `RAMIFY_READY` (a file that exists once the install finished).
- **Services** — for each: `<svc>_port` (the base; slot N adds N), `<svc>_dir`, `<svc>_health`
  (a path that answers only once it serves — check it exists), `<svc>_cmd`. A later service
  reads an earlier one's port as `$<SERVICE>_PORT`. Put the dependency first.
- **Sharing** — a slow-to-build service that most branches never touch gets `<svc>_shared_port`,
  `<svc>_shared_cmd` and `<svc>_shared_when` (the regex every changed path must match for the
  branch to reuse it). Migrations go in `<svc>_prepare`, which runs only for a private instance.
- **Wiring check** — `<svc>_smoke='<path> <codes regex>'`, e.g. the SPA proxying to the API.
- **QA recipe** — `RAMIFY_QA_PLAN` (the checklist glob, `<slug>` = worktree name),
  `RAMIFY_QA_RESULTS`, `RAMIFY_QA_NOTES` (seed data, test accounts, anything a reviewer needs).
- **Gates** — `RAMIFY_GATE_BEHIND=0` if the team pushes branches behind main on purpose.

## 3. Ask what only the human knows

Ask with `AskUserQuestion`, one focused question at a time, and only for what the repo does not
settle:

- Commit the config for the team, or keep it personal (`--local`)?
- Which services should be shared, and which paths mean a branch needs its own?
- Where the QA checklist lives, if the repo has no convention.

Recommend an answer for each one.

## 4. Write and prove it

```bash
RAMIFY_ROOT=<primary> ramify setup [--local] --force   # or edit the file by hand from the draft
RAMIFY_ROOT=<primary> ramify config                    # it loads, and shows what it resolved
RAMIFY_ROOT=<primary> ramify up                        # every service answers
RAMIFY_ROOT=<primary> ramify card
RAMIFY_ROOT=<primary> ramify down
```

A config is done when `up` prints every service `ready` with no `smoke` warning. Show the card
and the final file, and name any value you left unverified.

A committed config is sourced as bash by everyone who runs ramify in the repo. Review it like a
script.
