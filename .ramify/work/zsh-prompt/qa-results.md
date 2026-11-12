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
| Q23 | pass | Live, powerlevel10k: the user saw the line in the worktree, and the setup was checked in an interactive zsh (`prompt_ramify` defined, `_ramify_precmd` before `_p9k_precmd`). |
| Q24 | pass | Live, confirmed by the user with two Claude Code sessions (`claude --plugin-dir`). Codex is not installed on this machine, so the Codex half rests on Q2, Q12 and Q14. The sessions had ended when the agent looked, so the agent did not see the records. A hand run of the branch's `session-start` hook on this worktree wrote `qa1.session` (agent `claude`, the worktree, the Claude Code pid) and the name `aspen`. |
| Q33 | waived | The user waived the live check. Automated cases cover it: Q28 (header line), Q29 (the `ram` record), Q30 (`◆ ram`), Q31 (Sessions pane) and Q34 (health refresh). When the agent looked, the Ram Code that ran was main's build, not this branch's. |

Smoke run on the release build: `bin/ramify prompt` in this worktree printed `𖣂 zsh-prompt ⎇ feat/zsh-prompt · feature/standard 4/8 → qa`.

**Yield:** 33 of 34 cases ran, 33 passed, 0 failed, 0 fixed. The user waived 1 live case (Q33).
