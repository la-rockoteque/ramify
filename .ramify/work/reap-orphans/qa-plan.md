# Reap orphans QA plan

The automated cases are `check` lines in a new section of `test/smoke.sh`, "reap stops the stacks of deleted worktrees".
It uses its own throwaway repo `reapapp`, with one `python3 -m http.server` service on ports 17311-17315.

Fixtures:

- **ok**: a stack that serves.
- **gone**: a stack whose worktree is removed with `git worktree remove --force`.
- **zombie**: a stack whose server is killed, with a python sleep left in its group. `RAMIFY_ZOMBIE_AFTER=0`.
- The hook cases use the test's `RAMIFY_STATE_DIR` and `RAMIFY_REGISTRY`, so they never reap the machine.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `reap stops the stack of a removed worktree`: its port does not answer and its group is gone | build |
| Q2 | AC1 | medium | `reap removes the state of a removed worktree`: no `gone.env` and no `gone.*.pid` | build |
| Q3 | AC1 | low | `reap prints one line per stack it stops`: the output names `gone` | build |
| Q4 | AC2 | high | `reap keeps ok and zombie stacks`: both groups live afterwards | build |
| Q5 | AC3 | low | `reap with nothing to stop prints nothing`: a second run has empty output and exit 0 | build |
| Q6 | AC4 | high | `session start starts reap in the background`: after the hook returns, `reap.log` and the stamp exist within 10 s | build |
| Q7 | AC4 | medium | Read `on_session_start`: reap runs under `nohup … &` with stdin from `/dev/null`, and nothing waits for it | review |
| Q8 | AC5 | medium | `a second session start within 10 minutes does not reap`: the stamp's mtime does not change | build |
| Q9 | AC6 | medium | `RAMIFY_REAP=0 turns the session-start reap off`: with a fresh state dir, no stamp appears | build |
| Q10 | AC7 | high | Read `mem_project`: when `git worktree list` gives no worktree, it prints one line that names the project, and it lists and stops nothing. `PRIMARY` comes from the same command, so a failing list mostly stops ramify earlier. No fixture can break one call and not the other without a fake git. | review |
| Q11 | AC8 | low | `reap --help prints its usage` and `--help lists reap` | build |
| Q12 | — | low | Every script parses under `/bin/bash` 3.2 (the existing smoke check) | build |
| Q13 | AC4 | medium | On this machine, time the SessionStart hook with and without reap. The hook stays under 1 s longer. The build runs it by hand and records the numbers | build |

## Write set

```text
bin/ramify
hooks/ramify-hook
test/smoke.sh
README.md
.ramify/work/reap-orphans/*
# Deviation: the release bump to 0.10.2 ships reap to the installed plugin.
.claude-plugin/plugin.json
!libexec/workflow.py
!skills/*
!app/*
```
