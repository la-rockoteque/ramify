# QA results: hooks in Ram Code

The build cases ran with `cargo test` at commit 114b726: `tests/qa_hooks.rs` 27 passed,
`cli/tests/hooks.rs` 6 passed. The full suites of both crates pass.

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `pre_tool_deny_blocks_the_command` |
| Q2 | pass | `pre_tool_ask_counts_as_deny` |
| Q3 | pass | `exit_two_blocks_with_stderr` |
| Q4 | pass | `matcher_selects_the_tool` |
| Q5 | pass | `post_tool_context_reaches_the_model` |
| Q6 | pass | `prompt_block_sends_nothing` |
| Q7 | pass | `session_and_prompt_context_come_first` |
| Q8 | pass | `stop_block_gives_one_more_turn` |
| Q9 | pass | `stop_blocks_at_most_three_times` |
| Q10 | pass | `a_hook_past_its_timeout_is_killed` |
| Q11 | pass | `a_failing_hook_does_not_stop_the_turn` |
| Q12 | pass | `the_same_command_runs_once` |
| Q13 | pass | `every_source_runs_in_order` |
| Q14 | pass | `a_broken_source_is_skipped` |
| Q15 | pass | `enabled_plugin_hooks_run_with_plugin_root` |
| Q16 | pass | `hook_input_has_the_common_and_event_fields` |
| Q17 | pass | `a_deny_wins_over_an_allow` |
| Q18 | pass | `no_hooks_no_process` |
| Q19 | pass | `ramify_pr_gate_denies_in_ram_code` |
| Q20 | pass | `ramify_session_start_writes_the_ram_record` |
| Q21 | pass | `ram_code_no_longer_writes_its_own_record` |
| Q22 | waived in part | Live, Ram Code pid 5103 from the branch. The hook wrote `ram-5103.session` with agent `ram`. The prompt in the worktree showed `◆ spruce ⏸`. No PR opened. The user has no model provider set up, so no transcript showed the deny. The user said to assume it works. Q19 covers the deny. |
| Q23 | pass | audit: the README has the sources, events, decisions and `hooks/ram.json` |
| Q24 | pass | audit: `Hooked::none` starts no process; SessionStart runs on a thread; SessionEnd waits 5 s at most |
| Q25 | pass | `classify_each_action` |
| Q26 | pass | `classify_a_chain` |
| Q27 | pass | `classify_pr_details` |
| Q28 | pass | `classify_a_work_item_read` |
| Q29 | pass | `pre_workflow_deny_blocks_the_merge` |
| Q30 | pass | `post_workflow_gets_details_and_output` |
| Q31 | pass | `bridge_denies_for_claude_code` |
| Q32 | pass | `no_workflow_hooks_no_classifier` |
| Q33 | pass | audit: the README has the actions, the details, `ramify classify`, `ramify hooks run` and the bridge |
| Q34 | waived | The user has no model provider set up for Ram Code and said to assume it works. PR #52 stayed open after the merge request. Q29 and Q31 cover the deny in Ram Code and in the bridge. |

Fixed during QA: the installed ramify 0.10.2 plugin has no `hooks/ram.json`. Ram Code loaded its
Claude Code hooks and skipped the branch's `hooks/ram.json`. Commit 114b726 fixes it, with the
test `an_old_ramify_plugin_gives_way_to_ram_codes_own_hooks`.

**Yield:** 34 cases. 32 passed, 0 failed, 1 fixed during QA. The user waived Q34 and the
transcript half of Q22.
