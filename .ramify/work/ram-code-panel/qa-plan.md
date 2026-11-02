# QA plan: Ram Code side panel

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `panel_toggles_and_closes`: `Ctrl+P` opens the panel on the dashboard; `Ctrl+P` and `Esc` close it and leave the chat. | build |
| Q2 | AC2 | high | `dashboard_lists_worktrees`: a fake `ramify dash --json` feeds the pane; it lists the project, worktrees, slot and service state, and a later poll shows a changed state. | build |
| Q3 | AC3 | high | `approvals_approve_and_reject`: a fake `gates --json` lists a gate; `a` runs `approve` in that worktree; `r` with a note runs `reject --note`; both refresh the list. | build |
| Q4 | AC4 | medium | `scratch_lists_copies_and_removes`: items appear with their text; `c` copies through the clipboard command; `d` removes the pair. | build |
| Q5 | AC5 | high | `run_go_and_block`: pending runs appear; `g` runs `run go <id>` and the output is shown; `b` with a note runs `run block --note`. | build |
| Q6 | AC6 | medium | `panel_flag_opens_pane`: `--panel run` starts with the run pane selected. | build |
| Q7 | AC7 | low | `empty_states_name_the_command`: with no gates, scratch items or runs, each pane prints the command that creates one. | build |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`,
`.ramify/work/ram-code-panel/**`.

Must not change: `bin/**`, `cli/**`, `libexec/**`, `skills/**`, `hooks/**`, `app/**`,
`.claude-plugin/**`, `.codex-plugin/**`, unrelated `.ramify/work/**`.
