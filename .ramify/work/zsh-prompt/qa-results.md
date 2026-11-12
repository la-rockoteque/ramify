# QA results: zsh-prompt

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `session_start_writes_a_claude_record` passes (`cargo test`) |
| Q2 | pass | `session_start_with_codex_writes_a_codex_record` passes (`cargo test`) |
| Q3 | pass | `prompt_submit_and_stop_switch_the_state` passes (`cargo test`) |
| Q4 | pass | `state_change_without_a_record_writes_nothing` passes (`cargo test`) |
| Q5 | pass | `session_end_removes_the_record` passes (`cargo test`) |
| Q6 | pass | `prompt_input_exit_keeps_the_record` passes (`cargo test`) |
| Q7 | pass | `a_dead_pid_is_skipped` passes (`cargo test`) |
| Q8 | pass | `reap_removes_dead_session_records` passes (`cargo test`) |
| Q9 | pass | `no_record_without_a_ramify_config` passes (`cargo test`) |
| Q10 | pass | `prompt_matches_statusline_without_sessions` passes (`cargo test`) |
| Q11 | pass | `prompt_in_the_primary_checkout` passes (`cargo test`) |
| Q12 | pass | `two_sessions_in_since_order` passes (`cargo test`) |
| Q13 | pass | `a_session_in_another_worktree_is_not_shown` passes (`cargo test`) |
| Q14 | pass | `a_session_without_a_name_shows_its_agent` passes (`cargo test`) |
| Q15 | pass | `prompt_is_silent_outside_git` and `prompt_is_silent_without_a_config` passes (`cargo test`) |
| Q16 | pass | `prompt_survives_a_broken_record` passes (`cargo test`) |
| Q17 | pass | `prompt_json_has_statusline_fields_and_sessions` passes (`cargo test`) |
| Q19 | pass | `prompt_is_fast_with_twenty_worktrees` passes (`cargo test`) |
| Q20 | pass | `prompt_reads_the_health_cache` passes (`cargo test`) |
| Q21 | pass | `zsh_plugin_prints_the_prompt_line` passes (`cargo test`) |
| Q22 | pass | `zsh_plugin_refreshes_on_cd` passes (`cargo test`) |
| Q25 | pass | `prompt_install_links_the_plugin` passes (`cargo test`) |
| Q28 | pass | `header_shows_the_prompt_line` passes (`cargo test`) |
| Q29 | pass | `a_task_writes_a_ram_record` passes (`cargo test`) |
| Q30 | pass | `a_ram_session_shows_as_ram` passes (`cargo test`) |
| Q31 | pass | `sessions_pane_lists_live_records` passes (`cargo test`) |
| Q34 | pass | `ram_code_refreshes_the_health_cache` passes (`cargo test`) |
| Q18 | pass | Audit: no HTTP call is reachable in `Mode::Prompt`. `prompt_makes_no_http_request` passes. |
| Q26 | pass | Audit: the README sections exist. |
| Q27 | pass | Audit: both manifests register `UserPromptSubmit`. |
| Q32 | pass | `cargo test` at the root: 41 tests pass, 0 fail. |
| Q23 | pending | Live, in the user's terminal with powerlevel10k. |
| Q24 | pending | Live: Claude Code and Codex sessions in one worktree. |
| Q33 | pending | Live: Ram Code next to a Claude Code session. |

Smoke run on the release build: `bin/ramify prompt` in this worktree printed `𖣂 zsh-prompt ⎇ feat/zsh-prompt · feature/standard 4/8 → qa`.

**Yield:** 31 of 34 cases ran, 31 passed, 0 failed, 0 fixed. 3 live cases wait on the user.
