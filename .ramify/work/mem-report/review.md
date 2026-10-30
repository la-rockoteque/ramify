# Review: ramify mem

## Fixed

- `mem-project` called alone made a temp file and never removed it. It now removes it on exit.
- The tag scan matched its own command line, so it hid every runner in the caller's group. Fixed in the build.
- `--apply` printed "stopped" for a group the guard left alone. It now says "left alone".

## Open: decide before the PR

**R1: `finale` does not mean "nobody needs this stack".**
The work order names `finale` as the next step as soon as the PR is open.
On this machine, `runewarden/sbom-timeout` reached `finale` 27 minutes after its stack came up.
`runewarden/shard-retry` reached it after 7 minutes.
A reviewer can still need those stacks.

Proposal: split the verdict.

- `pr-open`: next step `finale`, branch not merged. Listed, never stopped by `--apply`.
- `merged`: the branch passes the merge test that `prune` uses (`merged_reason`). Stopped by `--apply`.

This changes AC4 and AC8 of the spec.

## Checked, nothing to change

- `--apply` reaches processes only through `kill_worktree`, behind the same tag guard as `down`.
- The "outside ramify" section has no signal path.
- Linux: `top -l`, `memory_pressure` and `hw.memsize` are missing. The values show `?` and the footprint falls back to RSS with a note.
- Runtime on this machine: about 7 s for 4 projects. Most of it is `ps -E` per project.
- Every script parses under `/bin/bash` 3.2.

## R1 decision

The user chose the split. `merged` uses `merged_reason`, the test of `prune`. `pr-open` is listed and never stopped. AC4, AC8, Q6, Q11 and Q12 changed to match.
