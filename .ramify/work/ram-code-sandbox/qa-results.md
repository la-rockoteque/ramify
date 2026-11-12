# QA results: Ram Code sandbox

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `sandbox_allows_writes_in_the_worktree` |
| Q2 | pass | `sandbox_allows_the_task_temp_folder` |
| Q3 | pass | `sandbox_denies_writes_outside` |
| Q4 | pass | `sandbox_denies_writes_through_a_symlink` |
| Q5 | pass | `sandbox_denies_the_network` |
| Q6 | pass | `sandbox_strips_keys` |
| Q7 | pass | `scrub_list_covers_every_provider_key` |
| Q8 | pass | `timeout_kills_the_process_group` |
| Q9 | pass | `timeout_is_clamped` |
| Q10 | pass | `timeout_keeps_partial_output` |
| Q11 | pass | `outside_sandbox_asks_ramify_run` |
| Q12 | pass | `tui_asks_without_the_window` |
| Q13 | pass | `run_ask_no_window_skips_the_window` (in `cli/tests/run_ask.rs`) |
| Q14 | pass | `pending_run_opens_the_run_pane` |
| Q15 | pass | `run_pane_highlights_the_script` |
| Q16 | pass | `blocked_run_reaches_the_model` |
| Q17 | pass | `no_sandbox_goes_to_the_human` |
| Q18 | pass | `no_decision_leaves_the_run_pending` |
| Q19 | pass after a fix | `audit.md`: credential reads and launches outside the sandbox were open. Fixed in `b5e6a80`, with `sandbox_hides_secret_files` and `sandbox_blocks_launching_outside`. |
| Q20 | waiting on the human | Needs a provider key and a person at the Run pane. |

Yield: 20 cases. 19 ran and passed, 0 failed. The audit fixed 2 gaps. Q20 waits on a live run.
