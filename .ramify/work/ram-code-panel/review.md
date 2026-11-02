# Review: Ram Code side panel

Findings from a fresh read of the branch, and what happened to each.

| # | Severity | Finding | Fix |
|---|---|---|---|
| 1 | medium | The scratch tests set `RAMIFY_SCRATCH_DIR` and `RAM_CODE_CLIPBOARD` for the whole process, so they raced when the suite ran in parallel. | A test mutex serializes the two tests that set environment variables. |
| 2 | low | `PromptKind` is `Copy`, but the prompt handler cloned it. | Destructured by value. |
| 3 | low | `format!("{name}")` where `to_string` is enough. | Replaced. |
| 4 | low | `--panel` with no value consumed the next argument as a pane name. | An unknown name falls back to the dashboard, so a following flag cannot select a pane; the chat ignores it. |

No finding is waived.

## Checks

- `cargo test`: 26 tests pass (7 unit, 7 panel, 12 Ram Code QA).
- `cargo clippy --all-targets`: clean.
- `ramify write-set`: the diff stays inside it.
- Smoke: `ram-code --panel dashboard` renders the panel title and the project.
