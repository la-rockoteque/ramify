# Verify: step-commands

CI (`.github/workflows/release.yml`) runs `cargo build --release --locked` for `cli/` and for
the root crate. It runs no tests. This branch runs every suite the repo has.

| Check | Result | Evidence |
|---|---|---|
| The failing tests from `diagnose.md` | pass | `cd cli && cargo test`: 32 parity tests pass, `step_records_and_gates_match_the_shell` and `step_guard_matches_the_shell` included. 11 unit tests pass. |
| CI build, `cli/` | pass | `cargo build --release --locked --manifest-path cli/Cargo.toml`: exit 0 |
| CI build, root crate | pass | `cargo build --release --locked`: exit 0 |
| Root crate tests | pass | `cargo test`: 37 pass, 0 fail |
| Lint | pass | `cargo clippy --all-targets` in `cli/` and at the root: exit 0, 0 warnings |
| `test/install.py` | pass | `OK`, on main and on the branch |
| `test/smoke.sh` | known | Main: 77 `FAIL` lines. Branch: 52. Each branch failure is also on main. The 25 that now pass cover `ask`, `reject`, `wait`, `approve`, `step na`, `step done` and the `pr-gate` hook. |
| `test/native_workflows.py` | known | Main: 26 failures and 1 error. Branch: 27 failures. The set of tests is the same. `test_auto_hook_requests_and_guard_hook_denies_source_edit` goes from an error to a failure. On the branch it gets further, then stops at `libexec/workflow.py`, which `525feea` deleted. |
| `test/repo_workflows.py` | known | 11 failures and 1 error on main and on the branch. |
| `test/codex.py` | known | 6 errors on main and on the branch. |
| `test/app_launch.py` | known | FileNotFoundError for `libexec/app-launch.py` on main and on the branch. |
| `test/skills.py`, `test/tickets.py` | known | `ModuleNotFoundError` for `yaml` and `jira` on main and on the branch. This machine does not have them. |

The legacy suites call the `libexec/` scripts that `525feea` deleted. Their port or removal is
a separate bug.
