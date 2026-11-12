# QA results: Ram Code deny-by-default sandbox

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `profile_starts_with_deny_default` |
| Q2 | pass | `unlisted_operation_fails` |
| Q3 | pass | `toolchains_work_in_the_sandbox` |
| Q4 | pass | `c_build_script_works_in_the_sandbox` |
| Q5 | pass | `git_works_in_the_sandbox` |
| Q6 | pass | `common_tools_work_in_the_sandbox` |
| Q7 | pass | `reads_outside_the_roots_fail` |
| Q8 | pass, with one fixture moved | `tests/qa_sandbox.rs` passes. In `sandbox_hides_secret_files`, the ordinary file moved from the fake home to the worktree. It was readable only because the fake home sat under the per-user temp folder, which review finding 4 closed. Every assertion stays. |
| Q9 | pass | `cannot_signal_outside` |
| Q10 | pass | `linked_worktree_git_reads_common_dir` |
| Q11 | pass | `sandbox_json_adds_paths` |
| Q12 | pass | `sandbox_json_cannot_unhide_secrets` |
| Q13 | pass | `project_sandbox_json_is_ignored` |
| Q14 | pass | `invalid_sandbox_json_is_an_error` |
| Q15 | pass | `denied_command_gets_the_hint` |
| Q16 | pass after a fix | `audit.md`: the Keychain daemon was reachable. Fixed in `66e148d`, test `keychain_is_out_of_reach`. |
| Q17 | pass | `audit.md` |
| Q18 | pass | The human ran the case live and reported it passed in the chat on 2026-11-12. |

Yield: 18 cases. 18 ran and passed, 0 failed. The audit fixed 1 gap.
