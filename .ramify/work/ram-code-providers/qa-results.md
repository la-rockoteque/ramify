# QA results: Ram Code providers

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `no_providers_file_keeps_env_behaviour` |
| Q2 | pass | `cargo test`: 60 tests pass, including every #53 test |
| Q3 | pass | `provider_list_marks_active_and_missing_keys` |
| Q4 | pass | `switching_to_deepseek_targets_its_endpoint` |
| Q5 | pass | `switch_is_saved_for_the_next_session` |
| Q6 | pass | `custom_provider_uses_its_url_key_and_model` |
| Q7 | pass | `custom_entry_overrides_a_preset` |
| Q8 | pass | `openai_request_maps_tools_to_functions` |
| Q9 | pass | `openai_reply_maps_back_to_content_blocks` |
| Q10 | pass | `openai_task_runs_a_tool_end_to_end` |
| Q11 | pass | `openai_bad_arguments_become_a_tool_error` |
| Q12 | pass | `unknown_provider_keeps_the_active_one` |
| Q13 | pass | `missing_key_names_the_variable` |
| Q14 | pass | `unreadable_providers_file_is_an_error_not_a_crash`, plus `worker_uses_presets_when_the_file_is_unreadable` |
| Q15 | pass | `env_var_overrides_saved_provider_for_one_run` |
| Q16 | pass | `model_command_sets_the_session_model` |
| Q17 | pass | `providers_file_never_holds_a_key` |
| Q18 | pass | Ram Code writes to disk in three places. `src/providers.rs` `Providers::save_active` writes `providers.json` with the `active` name only. `src/app.rs` `start_task_with_text` and `src/main.rs` `run_worker` write the message history, which holds no credential. `src/main.rs` `run_worker` writes events: the trace has the provider name, base URL and `Credential::label`, never the key. The key reaches only the request headers in `auth_headers` and `OpenAiLlm::complete`. |
| Q19 | pass | The human ran the case with real keys and approved it in the chat on 2026-11-12. |

Yield: 19 cases. 19 ran and passed, 0 failed. The audit fixed one gap (AC6, worker fallback).
