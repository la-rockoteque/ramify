# QA plan: ramify in Rust

## Fixtures

The parity tests use three fixture repos under `cli/tests/fixtures/`: one with no config, one
with two worktrees and no services, and one with a managed service. Each has a fixed
`STATE_DIR` so the goldens are stable. The goldens are captured from the shell CLI before the
port of each phase.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `binary_is_self_contained`: run a ported command with `PATH` pointing at an empty folder and `HOME` at a temp dir; assert it works and starts no shell or Python. | build |
| Q2 | AC2 | high | `dash_json_matches_shell_and_is_fast`: run the shell `ramify dash --json` on the fixtures, store the golden, run the Rust binary, assert byte-identical output and under 2 seconds. | build |
| Q3 | AC3 | high | `dash_project_json_matches_and_is_fast`: same golden check for one project, under 300ms. | build |
| Q4 | AC4 | high | `exit_codes_match_shell`: for every ported command, compare exit code and output on a success case, a usage error and a failure case. | build |
| Q5 | AC5 | high | `state_interoperates_with_shell`: start a worktree with the shell `up`, stop it with the Rust `down`, and the reverse; assert slot, env and PID files keep their layout. | build |
| Q6 | AC6 | high | `bin_delegates_and_falls_back`: run `bin/ramify` with a fake Rust binary for a ported command and with no fake for an unported one; assert delegation and fallback. | build |
| Q7 | AC7 | medium | Manual: open the approvals window, record a step gate, and check the status line and `reap` on session start. Expected: the same window, record and output as the shell CLI. | qa |
| Q8 | AC8 | high | Read the phase-6 diff: `libexec/*.sh` and `*.py` are gone, `bin/ramify` execs the binary, and Q2 to Q6 still pass. | audit |
| Q9 | AC9 | high | `parity_harness_covers_every_command`: the harness reads the usage table, asserts each command has a fixture, and compares stdout, stderr and exit code. | build |

## Write set

May change: `cli/**`, `bin/ramify`, `libexec/**`, `tests/**`, `README.md`, `.gitignore`,
`.ramify/work/rust-cli/**`.

Must not change: `skills/**`, `hooks/**`, `app/**`, `.claude-plugin/**`, `.codex-plugin/**`,
unrelated `.ramify/work/**`, `assets/**`.
