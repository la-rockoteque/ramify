---
name: worktree
description: Start a branch in its own git worktree so several can be worked at once — cut it from the remote main, copy what git does not bring, bootstrap it, label the session. Use before editing on a new branch, when asked to work on several branches in parallel, or when a session must not disturb another checkout.
argument-hint: "<slug>"
---

# One branch, one worktree

Two branches at once means two *working trees*. A `git checkout` under an agent that is
mid-edit is how an afternoon is lost.

## Cut it

```bash
ramify new <slug>
```

It fetches, then cuts `$RAMIFY_BRANCH_PREFIX<slug>` from `origin/$RAMIFY_MAIN` into
`$RAMIFY_WORKTREES/<slug>`. It copies `RAMIFY_COPY`, runs `RAMIFY_BOOTSTRAP` in the background,
and starts the QA stack after. `ramify config` shows the values. With no config, the defaults
are `main`, no prefix and `../<repo>-wt`.

- **Based on the remote main, never on this checkout's `HEAD`**: the other tree may sit on
  someone else's branch, and inheriting it drags their unmerged work into your PR.
- **Outside the repo**: a worktree inside the tree gets walked by every watcher and glob that
  does not read `.gitignore`.
- **Gitignored paths are copied with `--ignore-existing`**, excluding nested `worktrees/`. A
  plain `cp -R .claude` duplicates the agent harness's own worktrees into the new tree, and any
  test that scans the repo as a tree then fails in the suite and passes alone.
- The branch does **not** track main (`--no-track`), so `prune` can tell when its remote is gone.

## Label the session

`new` prints two lines: `/rename <slug>` and `/color <name>`. Give them to the human to paste —
both are built-ins an agent cannot run. The colour is hashed from the slug, so a tree keeps it
across sessions and `/clear`.

## Every command starts in the worktree

The path is the only thing that says which branch you edit. **Use the absolute worktree path in
every command**, including each one in a `&&` chain — an agent shell resets its directory
between tool calls. Before editing after a resume or a compaction, re-read
`git status --short --branch` *and* `pwd`.

## What is safe in parallel

- Type-check, lint and unit tests: per tree, safe.
- Dev servers: only through `ramify up`, which gives each tree its own port slot.
- Shared containers: one set for the machine. Two branches migrating the same local database
  must coordinate. `ramify` runs `<svc>_prepare` only for a branch that gets a private instance.

## Where parallel branches collide

The code rarely collides. The shared files nobody lists do: i18n bundles (two branches adding
keys before the same closing brace), cross-links between work items, and `CLAUDE.md` — everybody's
favourite file to append one line to. Re-run the parity or lint checks after resolving them.

## A red suite in a fresh tree

The tree was cut from the remote main, so a failing file your diff does not touch fails on main.
Name it, check it against the diff, and move on — no stashing, no bisecting.

## Before the push

The `behind` hook refuses a push while the branch is behind main: the PR would carry other
people's work and its green build would prove nothing about the merge. Merge main in, make the
tests pass on the result, then push. `RAMIFY_GATE_BEHIND=0` turns it off per repo.

To bound what a branch may touch, keep a write set (`path/glob` allowed, `!path/glob` immutable)
and run `ramify write-set <file>`.

## Tear it down

`ramify prune --apply` removes merged branches and their worktrees, stack stopped first.
`ramify delete <worktree>` removes one, merged or not. `git worktree list` is the truth about
what is open.
