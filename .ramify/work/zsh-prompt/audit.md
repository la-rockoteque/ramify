# Audit: zsh-prompt

Base: `origin/main`. The diff covers the spec and its Ram Code amendment (AC19–AC23). The build
step was recorded before the amendment. Commit `ba5301c` builds the amendment, and its tests are
part of this audit.

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `hooks/ramify-hook` `write_record`, `agent_pid`. Test `session_start_writes_a_claude_record`. |
| AC2 | met | `write_record` takes `RAMIFY_AGENT`. Test `session_start_with_codex_writes_a_codex_record`. |
| AC3 | met | `set_state` from `user-prompt-submit` and `on_stop`. Tests `prompt_submit_and_stop_switch_the_state` and `state_change_without_a_record_writes_nothing`. |
| AC4 | met | `on_session_end` removes the record after the `prompt_input_exit` check. Tests `session_end_removes_the_record` and `prompt_input_exit_keeps_the_record`. |
| AC5 | met | `statusline.rs` `agents` filters on `running`. `mem.rs` `reap_sessions`. Tests `a_dead_pid_is_skipped` and `reap_removes_dead_session_records`. |
| AC6 | met | `write_record` returns early unless `configured`. Test `no_record_without_a_ramify_config`. |
| AC7 | met | `prompt` and `statusline` share `render`. Tests `prompt_matches_statusline_without_sessions` and `prompt_in_the_primary_checkout`. |
| AC8 | met | `agents_line`, sorted by `since`. Test `two_sessions_in_since_order`. |
| AC9 | met | `agents` compares canonical roots. Test `a_session_in_another_worktree_is_not_shown`. |
| AC10 | met | Test `a_session_without_a_name_shows_its_agent`. |
| AC11 | met | `render` returns `None` with no root or no config. Tests `prompt_is_silent_outside_git`, `prompt_is_silent_without_a_config` and `prompt_survives_a_broken_record`. |
| AC12 | met | `render` adds `sessions` in `Mode::Prompt`. Test `prompt_json_has_statusline_fields_and_sessions`. |
| AC13 | met | In `Mode::Prompt`, `render` reads `cached_health` and never calls `http_status_path`. Test `prompt_makes_no_http_request`: a socket that accepts and never answers, and the prompt returns in under 1.5 s. A probe would wait 3 s. |
| AC14 | met | Test `prompt_is_fast_with_twenty_worktrees`: the median of 10 debug-build runs is 50 ms or less. |
| Behaviour 11 | partly met | `statusline` writes the health cache, but `card` does not. `card` runs per-service smoke checks, which measure something else than "`/` answers", so one cache file would mix two meanings. Raised for review: drop "and `card`" from the spec, or have `card` probe `/` too. |
| AC15 | met | `shell/ramify.plugin.zsh`. Tests `zsh_plugin_prints_the_prompt_line` and `zsh_plugin_refreshes_on_cd`. |
| AC16 | partly met | `shell/ramify.p10k.zsh`. Test `p10k_segment_draws_the_line_and_nothing_outside` uses a stand-in for `p10k segment`. A real powerlevel10k is QA case Q23. |
| AC17 | met | `prompt.rs` `install`. Test `prompt_install_links_the_plugin`. |
| AC18 | met | README: the command table, "The zsh prompt", the hooks list and the Ram Code section. |
| AC19 | met | `src/app.rs` `refresh_status`, `render_header`. Test `header_shows_the_prompt_line` in `tests/qa_prompt.rs`. |
| AC20 | met | `write_ram_record`, `task_ended`, `Drop for App`. Test `a_task_writes_a_ram_record`. |
| AC21 | met | `agents_line` maps `ram` to `◆`. Test `a_ram_session_shows_as_ram`. |
| AC22 | met | `src/panel.rs` `sessions`, `session_label`. Test `sessions_pane_lists_live_records`. |
| AC23 | met | `cargo test` at the root: every earlier test passes. |

## Audit cases

| Case | Verdict | Evidence |
|---|---|---|
| Q18 | pass | No call to `http_status_path` is reachable from `Mode::Prompt` in `statusline.rs` `render`. Test `prompt_makes_no_http_request`. |
| Q26 | pass | README sections as listed for AC18. |
| Q27 | pass | `.claude-plugin/plugin.json` and `hooks/codex.json` both register `UserPromptSubmit` with `ramify-hook user-prompt-submit`. |

## Write set

`ramify write-set .ramify/work/zsh-prompt/qa-plan.md`: the diff stays inside the write set.

## Tests that could pass for the wrong reason

- `session_start_writes_a_claude_record` checks that the pid runs. Under `cargo test`, the
  `agent_pid` walk can stop at the test process, not at an agent. The test proves that the pid
  is live, not that it is the agent's. QA case Q24 checks the real agent.
- `prompt_is_fast_with_twenty_worktrees` measures on this machine. A slow CI machine could fail
  it.
