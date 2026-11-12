# Review: Ram Code providers

A separate reviewer agent read the branch diff and the record. It ran `cargo test` and `cargo clippy --all-targets`, and edited nothing. Every finding below is fixed, with a test, except finding 9, which waits on your decision.

| # | Severity | Where | Failure | Fix | Test |
|---|---|---|---|---|---|
| 1 | high | `src/providers.rs` `find`, `resolve` | `{"name":"openai","model":"x"}` replaced the whole preset. With no `base_url` and no `key_env`, it fell into the `env` path and sent the Anthropic key as a Bearer token to the proxy. | A custom entry now changes only the fields it sets. The `env` path is chosen by name. | `partial_override_merges_with_the_preset` |
| 2 | high | `src/providers.rs` `resolve` | A custom provider with a `base_url` and no `key_env` got `ANTHROPIC_API_KEY` or the `ant` login token, sent to any host. | Only `env`, and `claude` at `https://api.anthropic.com`, use the Anthropic order. Any other provider without `key_env` is an error. | `a_provider_without_key_env_never_gets_the_anthropic_credential` |
| 3 | medium | `src/app.rs` worker spawn, `tests/qa_providers.rs` Q1 | Deleting the `RAM_CODE_PROVIDER`, `RAM_CODE_CONFIG_DIR` or `RAM_CODE_MODEL` lines in the spawn failed no test. Q1 compared `from_env` with itself. | A fake worker script records the three variables. Q1 now checks literal #53 values. A mutation run confirmed the new test fails without the `RAM_CODE_PROVIDER` line. | `worker_gets_the_active_provider_and_model`, `no_providers_file_keeps_env_behaviour` |
| 4 | medium | `src/openai.rs` `from_chat`, `assistant_message` | A refusal or empty reply stored an empty assistant turn. The next request then failed on every provider. | A refusal becomes text. An empty reply is an error with its `finish_reason`. An assistant turn without tool calls never sends `null` content. | `openai_empty_reply_is_an_error`, `openai_refusal_becomes_text`, `an_assistant_turn_with_only_text_never_sends_null_content` |
| 5 | medium | `src/app.rs` `switch_provider`, `src/openai.rs` `from_chat` | After a switch, the history kept another model's thinking signatures, and the API rejected them. A tool call without an id stored `null`. | `/provider` to another provider runs `strip_thinking` on the history. A missing id gets a fresh `call_…` id. | `switching_provider_strips_thinking_from_history`, `missing_tool_call_id_gets_one` |
| 6 | medium | `src/openai.rs` `chat_url` | A base with a path, such as `/v1beta/openai`, got an extra `/v1`. | A base with a path gets `/chat/completions` only. A bare host still gets `/v1/chat/completions`. | `chat_url_keeps_a_base_path` |
| 7 | low | `src/providers.rs` `save_active` | Any read error, such as bad UTF-8, counted as an empty file, and the save erased the custom providers. The write was not atomic. | Only a missing file starts from `{}`. The save writes `providers.json.partial`, then renames it. | `save_keeps_a_file_it_cannot_read` |
| 8 | low | `src/providers.rs` `check_base_url` | A `base_url` without a scheme became `http://`, so the key travelled in clear text. An empty host fell back to the local proxy. | A `base_url` must use `https`, or `http` to `localhost`, `127.0.0.1` or `[::1]`. A missing scheme or host is an error. | `a_base_url_must_be_https_or_loopback`, `a_custom_base_url_must_be_https` |
| 9 | low | `src/app.rs` `/model` | `openai` has no default model, and `/model` is not saved, so `openai` shows "not ready" in each new session. | **Waiver proposed.** The spec makes `/model` session-only, and Q16 holds to that. After fix 1, `{"name":"openai","model":"…"}` in `providers.json` sets a lasting default. Approving this review accepts the waiver. | — |
| 10 | low | `src/app.rs` header | The header resolved the provider on every frame, which read the file system. | The app keeps `provider_line` and refreshes it on load, `/provider` and `/model`. | covered by the existing app tests |

## Simplicity

- Removed `AgentConfig::from_process_env`, which no longer had a caller.
- `max_tokens_from_env` replaces the two copies of the `RAM_CODE_MAX_TOKENS` parsing.
- Kept `start_task(config)` as it is: it uses the config to show a resolve error before any work starts, and the existing tests call it.
- Kept `missing_key` beside `resolve`: the list must name the missing variable without failing on a missing model.

## Checks after the fixes

- `cargo test`: every suite passes, including 27 tests in `tests/qa_providers.rs` and 15 in the library.
- `cargo clippy --all-targets`: no warnings.
- `ramify write-set .ramify/work/ram-code-providers/write-set.txt`: the diff stays inside the write set.
