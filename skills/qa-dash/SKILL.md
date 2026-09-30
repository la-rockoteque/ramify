---
name: qa-dash
description: Show which QA stacks are running — one card for the current worktree, or a tile per worktree across the machine. Use when asked what is running, which port or URL a branch is on, whether the stack is up, or to clean up leaked stacks and merged branches.
argument-hint: "[all]"
---

# QA dashboard

Answer « where is it running » with the card, not with a paragraph.

```bash
RAMIFY_ROOT=<worktree> ramify card   # this worktree — the default
ramify dash                          # every claimed slot, plus containers and shared services
ramify watch [secs]                  # dash on a loop; for a human's terminal, never a turn
```

**Paste the output into the reply in a fenced block.** Running `card` and saying "here it is"
shows the human nothing.

- **`card`** answers questions about *this* branch: the URL, the port, whether it is up.
- **`dash`** answers questions about the machine: what runs, who holds the slots.

Print the card without being asked when you finish a change a human will look at. Do not print
it on a turn that changed nothing, and do not poll it.

## Reading a tile

```
┌──────────────────────────────────────────────────────────────────┐
│ feat-login                                                slot 1 │
│ story/feat-login *                                        up 16m │
├──────────────────────────────────────────────────────────────────┤
│ api shared   http://localhost:5100                         ready │
│ web          http://localhost:5174                         ready │
│ storybook    http://localhost:6007                      starting │
└──────────────────────────────────────────────────────────────────┘
```

- **`*`** after the branch: uncommitted changes — the reviewer sees more than is pushed.
- **`shared` / `private`**: whether the branch reuses the primary checkout's instance or runs its own.
- **`starting`**: the port is bound but does not answer yet (a build). **`down`**: nothing on it.
- A **`smoke`** row appears only when a service fails its wiring check. No row is the pass.
- **`slot N ORPHAN`**: a second `up` without a `down`. The older stack still holds the slot and
  `down` can no longer find it; `ramify cleanup` frees it.

## Housekeeping

- **`ramify cleanup`** — releases crashed claims, kills orphans, and stops stacks whose worktree
  was deleted. Touches nothing alive, never the containers or shared services, and refuses to
  run if it cannot read the worktree list.
- **`ramify prune [--apply]`** — deletes merged local branches and their worktrees. Dry run
  without `--apply`. Merged means: an ancestor of main whose remote is gone, content already in
  main, or main carries its merge commit (Bitbucket or GitHub wording) with nothing newer on the
  branch. Keeps dirty worktrees, the primary checkout and the current one. Never touches remotes.
- **`ramify complete <worktree> [--yes]`** — the branch is done: closes its Jira ticket (with
  `RAMIFY_JIRA_URL`), stops its stack, removes the worktree and the local branch. Refuses
  uncommitted or unpushed work. `prune --apply` and `cleanup` also close the tickets of merged
  branches; a ticket already done is left alone.
- **`ramify delete <worktree> [--yes]`** — force-deletes one worktree and its branch. Prints what
  is lost first. Needs `--yes` without a terminal.

`watch` has a menu on `m`: start or stop any worktree's stack, cleanup, prune, delete.
