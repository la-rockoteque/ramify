# Audit: Ram Code providers

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `src/app.rs` `App::load_providers` makes `env` active when the file is missing. `src/providers.rs` `resolve` hands `env` to `AgentConfig::from_env`. | `no_providers_file_keeps_env_behaviour`, and every #53 test passes unchanged |
| AC2 | met | `src/app.rs` `App::list_providers`, `src/providers.rs` `missing_key` | `provider_list_marks_active_and_missing_keys` |
| AC3 | met | `src/app.rs` `App::switch_provider`, `src/providers.rs` `Providers::save_active` and `preset("deepseek")` | `switching_to_deepseek_targets_its_endpoint`, `switch_is_saved_for_the_next_session` |
| AC4 | met | `src/providers.rs` `parse_entry`, `Providers::find`, `resolve` | `custom_provider_uses_its_url_key_and_model`, `custom_entry_overrides_a_preset` |
| AC5 | met | `src/openai.rs` `to_chat`, `from_chat`, `chat_url`, `OpenAiLlm`. `src/agent.rs` `run_turn` checks `INVALID_ARGUMENTS`. | `openai_request_maps_tools_to_functions`, `openai_reply_maps_back_to_content_blocks`, `openai_task_runs_a_tool_end_to_end`, `openai_bad_arguments_become_a_tool_error` |
| AC6 | met after a fix | `src/providers.rs` `unknown`, `resolve`, `resolve_from_env`. `src/app.rs` `App::load_providers`. | `unknown_provider_keeps_the_active_one`, `missing_key_names_the_variable`, `unreadable_providers_file_is_an_error_not_a_crash`, `worker_uses_presets_when_the_file_is_unreadable` |
| AC7 | met | `src/providers.rs` `Providers::active_name` | `env_var_overrides_saved_provider_for_one_run` |
| AC8 | met | `src/providers.rs` `Providers::save_active` writes only `active` | `providers_file_never_holds_a_key`, and Q18 below |

## Fixed during the audit

AC6 was partly met. With an unreadable `providers.json`, the app fell back to `env`, but the worker process returned the load error, so every task failed. `resolve_from_env` now falls back to the presets, as the app does. A custom provider still fails with the file's error. Test: `worker_uses_presets_when_the_file_is_unreadable`.

## Gaps and notes

- AC3: `switching_to_deepseek_targets_its_endpoint` checks the URL and headers through `messages_url` and `auth_headers`, not with a live send to `api.deepseek.com`. `AnthropicLlm::complete` uses those two functions, and `custom_provider_uses_its_url_key_and_model` proves the send path against a local server. The live check is Q19.
- The status line has no automated test. Q19 checks it on the live app.
- The build wrote the code before the tests, against the build step's test-first rule. Each test was run and passes for the criterion it names.
- Run the write-set check as `ramify write-set .ramify/work/ram-code-providers/write-set.txt`. With `qa-plan.md` as its argument, the checker reports every path as outside the set.

## Write set

`ramify write-set .ramify/work/ram-code-providers/write-set.txt`: the diff stays inside the write set.
