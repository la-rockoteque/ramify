# Diagnose: two clones share one state dir

## Symptom

`~/dev/rocko/gatekeeper` is an old clone of `security1/gatekeeper`. `~/dev/security/gatekeeper` is the live one.
Both resolve `STATE_DIR` to `/tmp/ramify/gatekeeper`.
`ramify dash` draws the live stacks twice. The old clone's copy shows each worktree on `main`.
Each command rewrites `$STATE_DIR/primary`, so the two clones take the dir from each other.

## Risk

`ramify cleanup` from the old clone reads its own worktree list.
The live worktrees are not in it, so cleanup stops every live stack and drops its state.

## Root cause

`bin/ramify` builds `STATE_DIR` from `RAMIFY_PROJECT` only.
`RAMIFY_PROJECT` defaults to the basename of the primary checkout.
Two checkouts with the same basename get the same dir.
`libexec/statusline.sh` builds the same path the same way.

## Second cause, same area

`test/native_workflows.py` runs the hook without `RAMIFY_STATE_DIR` and `RAMIFY_REGISTRY`.
Each run adds its temp repo to `~/.claude/ramify/projects` and makes a `/tmp/ramify/tmp*` dir.
The machine registry held 11 such temp paths.

## Failing tests

`test/smoke.sh`, section "two clones with the same directory name keep their own state":

- `same-named clones get separate state` failed.
- `the first clone stays the dir's primary` failed.

## Fix

- The first clone keeps `/tmp/ramify/<name>`.
- A second clone gets `/tmp/ramify/<name>-<8 hex of its path>` and keeps it.
- A claim whose checkout no longer exists is free again.
- `statusline.sh` applies the same rule.
- `native_workflows.py` sets `RAMIFY_STATE_DIR` and `RAMIFY_REGISTRY` to its temp dir.

Running stacks keep their dirs: the first clone to claim a name is the one that already uses it.

## Known failure, not in scope

`test/codex.py` `test_resume_preserves_unfinished_build` fails on `main` too.
The session-start hook makes the state dir during a read-only resume.
