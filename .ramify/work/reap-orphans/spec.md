# Spec: stop the stacks of deleted worktrees on their own

## Problem

A worktree removed outside ramify keeps its stack running.
On 2026-10-30 a moship stack ran for 46h after its worktree was deleted, and held 680 MB.
Only a manual `ramify cleanup` stops such a stack, and nobody runs it.
Nobody can use a stack whose worktree is gone, so ramify can stop it without asking.

A related defect is in 0.10.0. When `git worktree list` fails for a project, `ramify mem` calls each of its stacks `orphan`, and `mem --apply` stops them all.
`cleanup` refuses in that case. `mem` does not.

## Behaviour

- A new command, `ramify reap`, runs from anywhere, like `ramify mem`.
  It stops each stack, in every registered project, whose verdict in `mem` is `orphan`.
  It removes that stack's state, as `mem --apply` does.
  It prints one line per stack it stops, and nothing when there is none.
- The SessionStart hook starts `ramify reap` in the background and does not wait for it.
  The output goes to `/tmp/ramify/reap.log`.
- The hook starts it at most once per 10 minutes on the machine.
  A stamp file in the state root records the last start.
- `RAMIFY_REAP=0` in the environment turns the hook call off.
- When `ramify mem`, `mem --apply` or `reap` cannot read a project's worktree list, they leave that project's stacks alone.
  `mem` prints one line that says so.

## Acceptance criteria

- AC1: Given a running stack whose worktree was removed, `ramify reap` stops it and removes its state.
- AC2: `ramify reap` leaves `ok`, `zombie`, `merged` and `pr-open` stacks running.
- AC3: With nothing to stop, `ramify reap` prints nothing and exits 0.
- AC4: The SessionStart hook starts `ramify reap` in the background. The hook returns before reap finishes.
- AC5: A second session start within 10 minutes does not start reap again.
- AC6: With `RAMIFY_REAP=0`, the SessionStart hook does not start reap.
- AC7: Given a project whose worktree list cannot be read, `ramify mem` names that project and lists none of its stacks as `orphan`. `mem --apply` and `reap` stop none of its stacks.
- AC8: `ramify reap --help` prints its usage line, and `ramify --help` lists it.

## Out of scope

- Stopping `zombie` or `merged` stacks without a human. A person may still use them.
- The other work of `cleanup`: stale slot claims, strays, and closing Jira tickets.
- A launchd job that reaps without any session.

## Lane

Light. `reap` reuses the stop path of `mem --apply`. The guard in AC7 makes that path safer.
