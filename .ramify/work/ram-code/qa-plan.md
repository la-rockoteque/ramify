# QA plan: Ram Code

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `startup_lists_project_and_worktrees`: launch against a Ramify checkout and confirm project and worktree names appear from Ramify state. | build |
| Q2 | AC2 | medium | `selecting_worktree_shows_status_and_workflow`: select a listed worktree and confirm its branch, status and current workflow step appear. | build |
| Q3 | AC3 | high | `actions_call_ramify_with_selected_root`: use a fake Ramify executable to assert the selected root and expected arguments for create, stack start/stop, and workflow-step actions; confirm command output is shown. | build |
| Q4 | AC4 | high | `missing_project_state_shows_recovery`: launch outside a configured project and confirm an actionable empty state; simulate invalid state output and confirm a readable error state. | build |
| Q5 | AC5 | high | `command_failure_returns_to_interface`: make the fake Ramify executable exit nonzero and confirm the error is visible and the interface remains usable. | build |
| Q6 | AC6 | medium | `terminal_app_runs_without_electron`: build and launch the Rust binary in a pseudo-terminal with Electron unavailable; confirm the TUI starts and exits cleanly. | build |
| Q7 | AC7 | high | `coding_task_sends_tools_to_worktree`: a fake model returns a `write_file` call and then a final message; confirm the file lands under the selected worktree, the tool ran with that root, and progress is shown. | build |
| Q8 | AC8 | medium | `missing_api_token_shows_recovery`: start a task with no token and confirm an actionable message with the interface still usable. | build |
| Q9 | AC9 | high | `follow_up_task_keeps_early_context`: a fake model answers two tasks in one session; confirm the second request carries the first task and its answer. | build |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`, `.ramify/work/ram-code/**`. Two deviations: `.gitignore` (ignores `target/`) and `.ramify/work/ram-code/write-set.txt` (machine form of this list).

Must not change: `bin/**`, `libexec/**`, `skills/**`, `hooks/**`, `app/**`, `.claude-plugin/**`, `.codex-plugin/**`, unrelated `.ramify/work/**`.
