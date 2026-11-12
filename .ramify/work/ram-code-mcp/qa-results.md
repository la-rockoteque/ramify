# QA results: Ram Code MCP (stdio)

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `stdio_server_tools_are_offered_and_called` |
| Q2 | pass | `user_file_wins_over_project_file` |
| Q3 | pass | `config_variables_expand` |
| Q4 | pass | `unset_variable_fails_only_that_server` |
| Q5 | pass | `project_server_waits_for_approval` |
| Q6 | pass | `denied_project_server_never_starts` |
| Q7 | pass | `changed_entry_asks_again`, and `project_approval_binds_expanded_values` from the audit |
| Q8 | pass | `headless_skips_unapproved_servers` |
| Q9 | pass | `approval_is_stored_by_hash` |
| Q10 | pass | `approved_server_is_sandboxed` |
| Q11 | pass | `network_approval_keeps_other_rules`, and `keychain_is_out_of_reach_for_servers` from the audit |
| Q12 | pass | `npx_command_suggests_network` |
| Q13 | pass after a fix | `server_env_is_scrubbed`, `project_entry_cannot_read_secret_variables`, `user_entry_may_read_its_own_secrets` |
| Q14 | pass | `tools_list_follows_pages` |
| Q15 | pass | `tool_names_are_valid_and_unique` |
| Q16 | pass after a fix | `results_are_converted`, `mcp_results_escape_control_characters` |
| Q17 | pass | `large_result_is_cut` |
| Q18 | pass | `hung_call_times_out` |
| Q19 | pass | `crashed_server_restarts_once` |
| Q20 | pass | `no_server_outlives_the_task` |
| Q21 | pass | `pretooluse_hook_can_deny_an_mcp_call`, `hooks_see_mcp_tool_name_and_input` |
| Q22 | pass | `no_config_changes_nothing` |
| Q23 | pass | `both_formats_get_mcp_tools` |
| Q24 | pass | `http_entry_is_not_supported_yet` |
| Q25 | pass after fixes | `audit.md`: five gaps, fixed in `a0b56d4` and `1c5c00f` |
| Q26 | pass | The human ran the case live and reported it passed in the chat on 2026-11-12. |

Yield: 26 cases. 26 ran and passed, 0 failed. The audit fixed 5 gaps.
