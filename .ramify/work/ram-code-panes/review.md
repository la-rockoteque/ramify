# Review: Ram Code panes

| # | Severity | Finding | Fix |
|---|---|---|---|
| 1 | medium | The Sessions test expected the whole root path on one line, but the 40-column panel wraps it. | The test asserts the path's last segment, which stays on one line. |
| 2 | low | `and_then(|root| git_branch(root))` is a needless closure. | Passed the function directly. |
| 3 | low | `border_color` recomputes the selected session's colour on each render. | It hashes one short name; left as is. |

No finding is waived.

## Checks

- `cargo test`: 31 tests pass (7 unit, 7 panel, 6 panes, 12 Ram Code QA; the panel and panes files run together).
- `cargo clippy --all-targets`: clean.
- `ramify write-set`: the diff stays inside it.
