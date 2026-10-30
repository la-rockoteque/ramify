# ramify mem QA plan

The automated cases are `check` lines in a new section of `test/smoke.sh`, "mem reports and
reclaims". They use the throwaway `app` repo and its `python3 -m http.server` services.
`RAMIFY_ZOMBIE_AFTER=0` and `RAMIFY_RUNNER_AFTER=0` remove the waits, so no case sleeps for an hour.

Fixtures:

- **zombie**: a stack is up. The test kills its `http.server` and leaves the `sleep` in the same group alive.
- **pr-open**: a worktree whose work order has every step before `finale` recorded.
- **merged**: a worktree whose branch main merged with a GitHub merge commit.
- **orphan**: a stack is up, then its worktree directory is removed with `git worktree remove --force`.
- **untagged**: a fake `$T/bin/jest` script that sleeps, started with no `RAMIFY_TAG`. The orphan case double-forks it so its parent is pid 1. The live-parent case starts it as a plain background job.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | low | `mem prints swap and pressure`: output has a `swap` line with "used" and "of" | build |
| Q2 | AC2 | medium | `mem shows a footprint per stack`: the line of a running stack has a number followed by `MB` | build |
| Q3 | AC2 | medium | On this machine, compare `ramify mem` with `top -l1 -stats pid,mem` for one vite stack. The two values agree within 10%. The build runs it by hand and records the numbers | build |
| Q4 | AC3 | high | `mem calls a stack with a live group and dead ports a zombie` | build |
| Q5 | AC3 | high | `mem calls a stack that answers ok`: the healthy stack is never a zombie | build |
| Q6 | AC4 | medium | `mem calls a stack whose PR is open pr-open` and `mem flags a stack on a merged branch` | build |
| Q7 | AC5 | high | `mem flags a stack whose worktree is gone as orphan` | build |
| Q8 | AC6 | medium | `mem lists an orphaned untagged runner with its kill command` | build |
| Q9 | AC7 | medium | `mem lists a long untagged runner with a live parent` | build |
| Q10 | AC7 | low | `mem leaves a young runner out`: with `RAMIFY_RUNNER_AFTER=3600`, the live-parent runner is absent | build |
| Q11 | AC8 | high | `mem --apply stops zombie, merged and orphan stacks`: their ports and groups are gone afterwards | build |
| Q12 | AC8 | high | `mem --apply keeps an ok stack` and `mem --apply keeps a stack whose PR is open`: their ports still answer | build |
| Q13 | AC9 | high | `mem --apply never signals an untagged process`: both fake jest pids are alive afterwards | build |
| Q14 | — | medium | `mem with nothing to reclaim says so`: after `--apply`, a second run ends with "nothing to reclaim" | build |
| Q15 | AC10 | low | `mem --help prints its usage` and `--help lists mem` | build |
| Q16 | — | low | Every script parses under `/bin/bash` 3.2 (the existing smoke check) | build |
| Q17 | AC1–AC9 | medium | On this machine, run `ramify mem` and read it against `ramify dash` and `top`. Each verdict is right for the real stacks. The build runs it by hand | build |

## Write set

```text
bin/ramify
test/smoke.sh
README.md
skills/app/SKILL.md
.ramify/work/mem-report/*
# Deviation: the branch merges fix/clone-state-collision (#28). Without it, `mem --apply` stops
# the live stacks of a project when an old clone with the same name is registered.
.ramify/work/clone-state-collision/*
libexec/statusline.sh
test/native_workflows.py
# Deviation: the release bump to 0.10.0 ships #28 and mem to the installed plugin.
.claude-plugin/plugin.json
!libexec/workflow.py
!hooks/*
!app/*
```
