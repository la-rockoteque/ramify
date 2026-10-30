# Spec: ramify mem

## Problem

On 2026-10-30 this 16 GB machine had 13.3 GB of 14.3 GB swap in use.
The waste was hard to see:

- `ramify dash` showed a QA stack as "6 MB". `ps` RSS leaves out compressed and swapped pages. Its real footprint was 500-850 MB.
- Two moship `dotnet watch` stacks had their API down for 46h. Their watchers held 680 MB each.
- One of those stacks belonged to a worktree deleted days before. Only a manual `ramify cleanup` released it.
- Five stacks belonged to branches at the `finale` step. Nobody QAs those any more.
- A jest run held about 20 GB in 10 workers. Nothing outside `top` showed it.

A human or an agent needs one command that shows where the memory goes and what ramify can take back.

## Behaviour

`ramify mem` runs from anywhere, like `ramify dash`. It prints four sections.

1. **Machine**: physical RAM, swap used and total, and the free percentage from `memory_pressure`.
2. **Stacks**: one line per running stack of every registered project. It shows the project, the worktree, the footprint in MB, the uptime and a verdict:
   - `ok`: every service answers.
   - `zombie`: a process group of the stack is alive, no service port answers, and the stack has been up for more than 1 hour.
   - `finale`: the branch's work order names `finale` as its next step.
   - `orphan`: the worktree no longer exists.
3. **Outside ramify**: dev servers, watchers and test runners that carry no ramify tag. It lists the ones that are orphaned (parent pid 1) and test runners that have been running for more than 30 minutes. Each line shows the pid, the footprint, the age, the working directory and the `kill` command to stop it.
4. **Build servers**: the existing build-servers line.

Footprint is the `MEM` column of `top -l 1`, summed over the process group. It counts compressed memory. Build servers are left out of a stack's sum, as in `dash`.

`ramify mem --apply` stops every `zombie`, `finale` and `orphan` stack with the same code that `down` and `cleanup` use. It prints one line per stack it stops. It never signals a process without a ramify tag. The "outside ramify" section stays a report.

States:

- No registered project: the same message as `dash`.
- Nothing to take back: the last line says "nothing to reclaim".
- `top` or `memory_pressure` not available (Linux): those values show as `?`, and footprint falls back to RSS with a note.

## Acceptance criteria

- AC1: `ramify mem` prints the swap used and total, and the memory free percentage.
- AC2: Given a running stack, its line shows a footprint from `top`, not from RSS.
- AC3: Given a stack whose process group is alive, whose ports do not answer, and whose uptime is over 1 hour, its verdict is `zombie`.
- AC4: Given a stack whose work order's next step is `finale`, its verdict is `finale`.
- AC5: Given a stack whose worktree was deleted, its verdict is `orphan`.
- AC6: Given an untagged dev server or test runner with parent pid 1, it shows under "Outside ramify" with its pid and a `kill` command.
- AC7: Given an untagged test runner older than 30 minutes with a live parent, it shows under "Outside ramify".
- AC8: `ramify mem --apply` stops the `zombie`, `finale` and `orphan` stacks, and leaves `ok` stacks running.
- AC9: `ramify mem --apply` never signals an untagged process.
- AC10: `ramify mem --help` prints its usage line. The command list in `ramify --help` includes it.

## Out of scope

- A `--json` output and a memory panel in the desktop app.
- Running `cleanup` on its own from a hook. Deleted worktrees keep their stacks until someone runs it. A follow-up can decide that.
- Limiting jest or vitest workers in a repo.

## Open questions

- Should the 1 hour zombie threshold and the 30 minute test-runner threshold be config values (`RAMIFY_ZOMBIE_AFTER`, `RAMIFY_RUNNER_AFTER`)? Proposed: yes, as environment variables with these defaults.

## Lane

Light. The command is new and reads more than it writes. `--apply` reuses the stop code that is already tested.
