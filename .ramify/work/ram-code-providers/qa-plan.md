# QA plan: Ram Code providers

The repo declares no services, so `ramify up` starts nothing. The build settles the automated cases with `cargo test`. Tests point a provider at a local fake HTTP server and record each request. Tests set `RAM_CODE_CONFIG_DIR` to a temporary directory, so they never read or write the real `~/.config/ram-code`.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `no_providers_file_keeps_env_behaviour`: no `providers.json`, `ANTHROPIC_AUTH_TOKEN` and `ANTHROPIC_BASE_URL` set. The active provider is `env`, and the URL, headers and model equal the #53 values. | build |
| Q2 | AC1 | high | The #53 tests in `src/agent.rs` and `tests/qa_plan.rs` pass unchanged. | build |
| Q3 | AC2 | medium | `provider_list_marks_active_and_missing_keys`: with a custom provider and only `DEEPSEEK_API_KEY` set, `/provider` lists `claude`, `openai`, `deepseek`, `env` and the custom one. It marks the active one, and marks `openai` as missing a key. | build |
| Q4 | AC3 | high | `switching_to_deepseek_targets_its_endpoint`: `/provider deepseek`, then a task. The fake server sees `/anthropic/v1/messages` with `x-api-key` equal to `DEEPSEEK_API_KEY`. | build |
| Q5 | AC3 | high | `switch_is_saved_for_the_next_session`: after `/provider deepseek`, `providers.json` has `"active": "deepseek"`, and a new config load selects `deepseek`. | build |
| Q6 | AC4 | high | `custom_provider_uses_its_url_key_and_model`: a custom Anthropic-format entry. The request goes to its base URL with the key from its key variable and its model. | build |
| Q7 | AC4 | medium | `custom_entry_overrides_a_preset`: a custom entry named `deepseek` with another base URL. Its URL wins. | build |
| Q8 | AC5 | high | `openai_request_maps_tools_to_functions`: an Anthropic request with a system prompt, text, `tool_use` and `tool_result` maps to Chat Completions. The system prompt is a `system` message, tools are `function` entries, a tool call is an assistant `tool_calls` entry, and a result is a `tool` message with the same id. | build |
| Q9 | AC5 | high | `openai_reply_maps_back_to_content_blocks`: a Chat Completions reply with text and two `tool_calls` maps to one `text` block and two `tool_use` blocks. Arguments are parsed from their JSON string. | build |
| Q10 | AC5 | high | `openai_task_runs_a_tool_end_to_end`: an OpenAI-format provider against the fake server. The first reply calls `read_file`, the second replies with text. The tool runs, the second request carries a `tool` message, and the final text is the answer. The request goes to `/v1/chat/completions` with `Authorization: Bearer`. | build |
| Q11 | AC5 | medium | `openai_bad_arguments_become_a_tool_error`: a `tool_calls` entry with arguments that are not JSON. The loop returns an error result to the model and does not crash. | build |
| Q12 | AC6 | high | `unknown_provider_keeps_the_active_one`: `/provider nope` shows an error that lists the valid names. The active provider and `providers.json` do not change. | build |
| Q13 | AC6 | high | `missing_key_names_the_variable`: active `openai` without `OPENAI_API_KEY`. Starting a task shows an error naming `OPENAI_API_KEY`, and the interface stays usable. | build |
| Q14 | AC6 | medium | `unreadable_providers_file_is_an_error_not_a_crash`: `providers.json` with invalid JSON. Ram Code starts on `env` and shows an error naming the file. | build |
| Q15 | AC7 | medium | `env_var_overrides_saved_provider_for_one_run`: `providers.json` says `deepseek` and `RAM_CODE_PROVIDER=openai`. The active provider is `openai`, and the file still says `deepseek`. | build |
| Q16 | AC6 | low | `model_command_sets_the_session_model`: `/model x` changes the model of the next request and does not write `providers.json`. | build |
| Q17 | AC8 | high | `providers_file_never_holds_a_key`: after a switch with all keys set, no key value appears in `providers.json`. | build |
| Q18 | AC8 | high | Read the diff: no code path writes a key value or a token to disk or to the worker's events file. | audit |
| Q19 | AC3, AC5 | medium | With real keys: switch to `deepseek`, then to `openai`, and run "list the files in src" on each. Each answers, and the status line shows the provider and model. | qa |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`, `.ramify/work/ram-code-providers/**`.

Must not change: `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`, `.codex-plugin/**`, other `.ramify/work/**` folders.

## Still open from the spec

- The default model for `openai`. Until you name one, `openai` has no default model. Starting a task without `RAM_CODE_MODEL` or `/model` shows an error that asks for one.
- The LiteLLM URL and format. Until you give them, LiteLLM is a custom provider in `providers.json`, and the README shows an example entry.
