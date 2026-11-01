# Audit: Ram Code

Base: `3a8a269` (`origin/main`). Branch diff: 16 files, 4642 insertions.

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `src/app.rs` `App::load` parses `ramify dash-project --json` with `src/state.rs` `parse_project` and sets `Screen::Main`. Test `startup_lists_project_and_worktrees`. |
| AC2 | met | `src/app.rs` `open_selected` and `detail_lines` show branch, dirty state, services and workflow next step. Test `selecting_worktree_shows_status_and_workflow`. |
| AC3 | met | `src/app.rs` `invoke` calls `run_command` with `CommandSpec { root: worktree.path, args: action.args() }`; `src/ramify.rs` `RealRunner` sets `RAMIFY_ROOT`. Tests `actions_call_ramify_with_selected_root` and `command_failure_returns_to_interface`. |
| AC4 | met | `App::load` sets `Screen::Empty` when no worktree matches the root, and `Screen::Error` on a failed or invalid read. Test `missing_project_state_shows_recovery`. |
| AC5 | met | `invoke` stores the failed output and `Screen::Message`; `back` reloads to `Main`. Test `command_failure_returns_to_interface`. |
| AC6 | met | `src/main.rs` uses Ratatui and Crossterm only. Test `terminal_app_runs_without_electron` runs the binary under a pseudo-terminal with Electron off `PATH`. |
| AC7 | met | `src/agent.rs` `run_turn` offers `read_file`, `write_file`, `list_files` and `run_command` from `src/tools.rs`, all rooted at the worktree. Test `coding_task_sends_tools_to_worktree`. The real endpoint was also smoke-tested. |
| AC8 | met | `App::start_task` sends `AgentConfig` errors to `Screen::Message`; the message names `ANTHROPIC_AUTH_TOKEN`. Test `missing_api_token_shows_recovery`. |
| AC9 | met | `run_turn` takes the earlier messages and returns the new history; `App` keeps `history` and `finish_task` reads it from the worker. Test `follow_up_task_keeps_early_context`. The chat smoke test showed two turns in one transcript. |

## QA plan cases

Every case Q1 to Q9 is marked `build` in `qa-plan.md`. No case is settled by audit.

## Write set

`ramify write-set .ramify/work/ram-code/write-set.txt` reports the diff stays inside it.
The two deviations are declared in `qa-plan.md`: `.gitignore` and the machine write set.

## Notes

- The plan was extended after the build record: AC7 to AC9 and Q7 to Q9 cover the coding
  loop and the chat history. The user directed the extension.
- Q6 sends `Ctrl+C` to quit because `q` is typed into the chat input.
- `App::load` calls `dash-project` rather than `dash`: `dash` took 49 seconds on this machine.
