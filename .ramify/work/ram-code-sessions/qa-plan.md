# QA plan: Ram Code sessions

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `sessions_group_by_project`: two projects with sessions; headers show each project and count, and only the open group's sessions render. | build |
| Q2 | AC2 | high | `opening_a_group_closes_the_others`: `Enter` on a closed header opens it and closes the open one; `Space` does the same. | build |
| Q3 | AC3 | high | `enter_on_a_session_opens_its_root`: `Enter` on a session row sets the chat root and reloads the project. | build |
| Q4 | AC4 | medium | `agent_stop_runs_down`: `x` in the Agents pane runs `down` with the selected worktree root and shows the output. | build |
| Q5 | AC5 | high | `other_panes_keep_working`: dashboard, approvals, scratch, run and artifacts keep their empty states and keys. | build |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`,
`.ramify/work/ram-code-sessions/**`.

Must not change: `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`,
`.codex-plugin/**`, unrelated `.ramify/work/**`.
