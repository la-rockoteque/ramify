# Audit: Ram Code side panel

Base `origin/main`. Branch diff: `src/panel.rs`, `src/app.rs`, `src/main.rs`, `src/lib.rs`,
`tests/qa_panel.rs`.

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `App::on_key` handles `Ctrl+P`; `Panel::close` on `Esc`. Test `panel_toggles_and_closes`. |
| AC2 | met | `Panel::refresh` runs `dash --json`; `Panel::lines` lists project, worktrees, slot and services; `App::tick` refreshes every 5s. Test `dashboard_lists_worktrees`. |
| AC3 | met | `Panel::approve` runs `approve <step>` in the gate path; the `r` prompt collects a note and runs `reject <step> --note`. Test `approvals_approve_and_reject`. |
| AC4 | met | `Panel::refresh` reads the scratch directory; `copy` pipes to `pbcopy` (or `RAM_CODE_CLIPBOARD`); `delete_scratch` removes the pair. Test `scratch_lists_copies_and_removes`. |
| AC5 | met | `go_run` runs `run go <id>` and shows the tail; `b` collects a note and runs `run block <id> --note`. Test `run_go_and_block`. |
| AC6 | met | `main` parses `--panel <pane>` and `--panel=<pane>` and calls `Panel::open_on`. Test `panel_flag_opens_pane`; smoke run with `--panel dashboard`. |
| AC7 | met | Each pane prints the command that creates data. Test `empty_states_name_the_command`. |

Write set: `ramify write-set` reports the diff stays inside it.

## Notes

- The panel uses the existing runner, so tests drive it with a fake CLI.
- The scratch tests take a lock because they set process environment variables.
