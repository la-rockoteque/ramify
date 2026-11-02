# QA plan: Ram Code panes

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `sessions_list_names_roots_branches`: session files in a temp state directory feed the pane; the row shows the id, name, root and branch. | build |
| Q2 | AC2 | medium | `selected_session_colour_themes_panel`: the selected row and the panel border use the MD5 palette colour of the session name. | build |
| Q3 | AC3 | high | `enter_opens_session_root`: `Enter` on a session sets the chat root to that session's root and reloads the project for it. | build |
| Q4 | AC4 | medium | `agents_list_the_worktree_services`: pid files in the state directory list the worktree's services; the empty state names `ramify up`. | build |
| Q5 | AC5 | high | `artifacts_list_and_preview`: a worktree with an order lists its documents and shows the selected one's first lines; without an order it names `ramify new`. | build |
| Q6 | AC6 | high | `existing_panes_still_work`: the dashboard, approvals, scratch and run panes keep their empty states and keys. | build |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`,
`.ramify/work/ram-code-panes/**`.

Must not change: `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`,
`.codex-plugin/**`, unrelated `.ramify/work/**`.
