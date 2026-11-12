# Audit: step-commands

Criteria from [diagnose.md](diagnose.md): the symptom is gone, the failing tests pass for the
right reason, and the diff stays inside the fix plan.

| Criterion | Verdict | Evidence |
|---|---|---|
| The step commands no longer print the usage and exit 2 | met | `cli/src/main.rs` routes `step <sub>` (except `asked`) to `steps::step`, and `approve` and `reject` to `steps::decide`. A manual run on a fresh fixture, with each result checked by hand: `step gate --pr` exits 1 and names the missing steps. `step done spec` refuses a human gate without `--approved`. `step ask`, `reject` without and with `--note`, `step wait`, `approve --note`, `step done --approved` and `step na --note` print what `workflow.py` printed. |
| The work order gets the same frontmatter as before | met | `steps.rs` `Head` changes one top-level key and keeps the other lines. The manual run wrote `- {step: build, date: '2026-11-12', na: true, note: 'n/a: none'}` and the `ask:` block in PyYAML form. The parity test compares the final `order.md` byte for byte. |
| The PR gate denies again | met | `steps.rs` `gate` exits 1 while a step before the `opens_pr` step is missing. `on_pr_gate` in `hooks/ramify-hook` denies on exit 1. |
| The edit guard holds again | met | `guard.rs` ports `native_guard.py`. The parity test `step_guard_matches_the_shell` covers `auto_ask` off and on. A manual run with `auto_ask: true` and `spec` next denied an `Edit` of `src/main.rs` and a `cargo build`. It allowed an `Edit` of `.ramify/work/demo/spec.md` and `git status 2>&1 && ramify step`. |
| `ramify gates` sees an open request | met | `Head::write` calls `commands::check`, which writes or drops the index entry. The manual run shows one entry after `step ask` and none after `approve`. |
| `step_records_and_gates_match_the_shell` passes for the right reason | met | It ran the shell and the binary on separate fixtures. Exit codes, stdout, stderr and the final `order.md` are equal. The run above shows that the outputs are real results, not two identical errors. |
| `step_guard_matches_the_shell` passes for the right reason | met | With `auto_ask` on, both CLIs print the same deny JSON. The manual run shows a deny. |
| The diff stays inside the fix plan | met | New `cli/src/steps.rs` and `cli/src/guard.rs`. `cli/src/main.rs` adds the routes. `cli/src/workflow.rs` exposes fields and parses `auto_ask`, `planning_paths`, `opens_pr`, `ask.note` and `ask.digests`. `cli/src/commands.rs` makes `check` `pub(crate)`. There is no refactor and no rename. |
| The plugin's binary is rebuilt | partly met | The plugin installs the binary from a release, so this branch cannot rebuild it. Until the release, the `zsh-prompt` work order uses this worktree's binary: `cli/target/debug/ramify`. |

## Gaps left for a later branch

- `step asked` without `--json` still prints the usage. `workflow.py` ignored the flag.
- The native repository-workflow route (`repo-workflows.py`, for `RAMIFY_WORKFLOW_SOURCE=repo`
  without `.ramify/workflows.yml`) has no port, for `step` or for the step commands.
- `check_definitions` and the native record checks of `workflow.py` have no port. A broken
  `workflows.yml` gives a serde error, not the old message.
