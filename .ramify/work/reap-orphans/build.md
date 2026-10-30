# Build notes: reap orphans

## Automated cases

Q1 to Q6, Q8, Q9, Q11 and Q12 are `check` lines in the "reap stops the stacks of deleted worktrees" section of `test/smoke.sh`. All pass.

## Read in the code

- **Q7**: `reap_in_background` starts `nohup "$RAMIFY" reap … </dev/null &` and `disown`s it. Nothing waits for it.
- **Q10**: `mem_project` counts the worktrees in `git worktree list`. At zero, it prints one `unreadable` line naming the project and returns. It lists and stops nothing. `mem` does not count that line as something to reclaim.

## Run by hand

- **Q13**: the SessionStart hook on this machine took 1.1 to 1.2 s with reap and 1.3 to 1.8 s without. The difference is noise.
- A real `ramify reap` on this machine found nothing to stop, in 10 s.

## Found during the build

- The "one line" check looked for one space before `stopped`. reap prints two. The check was wrong, not reap.
- Two smoke runs at once broke each other: the ports and fixture names are fixed. `test/smoke.sh` now takes a lock in `/tmp/ramify-smoke.lock`, a fixed path, since a sandboxed shell has its own `$TMPDIR`.
