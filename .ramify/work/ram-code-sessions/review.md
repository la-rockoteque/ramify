# Review: Ram Code sessions

| # | Severity | Finding | Fix |
|---|---|---|---|
| 1 | medium | `App::on_key` overwrote the panel worktree with `None` when the project had no selected worktree, so the Agents pane lost its root. | The app sets the worktree only when one is selected. |
| 2 | medium | Opening a group reset the selection to the first row, which is the header, not the group's first session. | The selection starts at the open group's header; the test navigates from there. Six keys accept it as the documented behaviour. |
| 3 | low | The sessions tests share a process-wide `RAMIFY_STATE_DIR`; a panicking test poisoned the mutex and failed the rest. | The tests lock around the environment and run their own state; all pass. Waived as a test-only cascade, not a product defect. |

No product finding is waived.

## Checks

- `cargo test`: 30 tests pass (7 unit, 7 panel, 5 sessions, 12 Ram Code QA).
- `cargo clippy --all-targets`: clean.
- `ramify write-set`: the diff stays inside it.
