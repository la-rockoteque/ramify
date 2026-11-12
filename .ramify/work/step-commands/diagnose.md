# Diagnose: the step commands are missing from the Rust CLI

## Symptom

Since 0.10.x, every ramify workflow stops at its first step. These commands print the full
usage and exit 2:

- `ramify step done|na|gate|ask|wait|guard|ensure`
- `ramify approve`
- `ramify reject`

To reproduce, run this in a worktree that has a work order:

```
$ ramify step ask spec
usage: ramify <command> [--help]
  setup [--print | --force] [--local]      write .ramify.conf for this repo
  ...
$ echo $?
2
```

`ramify step` with no argument and `ramify step asked --json` still work.

## Risk

- No agent can record a step or put a human gate up. Every work order on every machine is stuck.
- The PR gate fails open. The `pr-gate` hook denies only on exit 1 from `step gate --pr`. Exit 2
  means "broken order" to it, so `gh pr create` goes through with steps missing.
- The edit guard fails open. With `auto_ask: true`, the `human-gate` hook pipes each tool call
  into `step guard`. That now prints the usage to stdout and denies nothing, so an agent can
  edit code while a human gate waits.
- The human cannot use `ramify approve` or `ramify reject` from a terminal.

## Root cause

Commit `525feea` ("Make the Rust binary the ramify CLI") deleted the bash CLI and `libexec/`.
Before it, `bin/ramify` handed the step commands to `libexec/workflow.py` (the
`step) case "${2:-}" in done|na|gate|wait|asked|guard) wf …` arm, now in
`cli/tests/reference/bin/ramify:1703`). The port had not reached these commands. In
`cli/src/main.rs`, only `step` alone (line 196) and `step asked --json` (line 184) have a
route. Every other `step …`, `approve` and `reject` falls through to `print_usage(None)` and
`exit(2)` (line 203).

The parity suite had no case for these commands, and CI (`release.yml`) runs no tests, so
nothing caught the gap.

## Failing tests

In `cli/tests/parity.rs`:

- `step_records_and_gates_match_the_shell` runs one sequence through the bash reference and
  through the binary. The sequence covers `step gate --pr`, `step gate <step>`, `step done`,
  `step done --approved`, `step na --note`, `step ask`, `step asked --json`, `reject` without
  and with `--note`, `approve --note`, `step wait`, an unknown step, `step ensure` and `step`.
  It compares exit codes, stdout, stderr and the final `order.md`. It fails now at
  `step gate --pr`: exit 1 from the shell, 2 from the binary.
- `step_guard_matches_the_shell` pipes an `Edit` tool call into `step guard`, with `auto_ask`
  off and on. It fails now with auto_ask off: the shell prints nothing and exits 0. The binary
  prints the usage and exits 2.

## Fix

- Port the rest of `workflow.py` to `cli/src/workflow.rs`: `record` (done and na), `gate`,
  `ask`, `approve` and `reject` (`cmd_decide`), `wait`, `guard` and `ensure`. Keep the same
  messages, exit codes and frontmatter writes.
- `step ask` and `step ensure` open the approvals window as `bin/ramify` did, unless
  `RAMIFY_POPUP=0`. The window code is already in Rust (`ramify approvals`).
- Route the commands in `cli/src/main.rs`.
- Rebuild the binary that the plugin runs, so that the open `zsh-prompt` work order can go
  through its spec gate.

## Known failures, not in scope

Main has these failures too. The branch has added only the two tests above.

- `test/*.py` and `test/smoke.sh` call `libexec/`, which `525feea` deleted.
  - `app_launch.py`: FileNotFoundError.
  - `codex.py`: 6 errors.
  - `native_workflows.py`: 26 failures, 1 error.
  - `repo_workflows.py`: 11 failures, 1 error.
  - `smoke.sh`: 77 FAIL lines.
- `skills.py` and `tickets.py` need the `yaml` and `jira` modules, which are not installed here.
- These suites must move to the Rust CLI or go away. That is a separate bug.
- CI runs no test suite. Adding `cargo test` to CI is also separate. It is the change that
  would have caught this bug.
