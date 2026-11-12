# QA plan: Ram Code streaming and interrupt

The repo declares no services. The build settles the automated cases with `cargo test`, in `tests/qa_streaming.rs`. The tests run a local fake HTTP server on `127.0.0.1`, like the one in `tests/qa_providers.rs`. It streams Anthropic SSE events or OpenAI chunks with set pauses, records each request body, and can answer `application/json`. Interrupt cases use the agent loop with a cancel flag, a fake worker script, and the fake `ramify` from `tests/qa_sandbox.rs`. Cases that run a real sandbox are `#[cfg(target_os = "macos")]`. No case uses a real provider. No new dependency is planned: `ureq` already reads a body as a stream.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `text_deltas_reach_the_events_file_before_finished`: the fake server pauses 300 ms between three `text_delta` events. The worker's events file has `text_delta` lines before `finished`. | build |
| Q2 | AC1 | medium | `transcript_shows_partial_text`: with a fake worker that writes deltas slowly, `App::poll_events` puts the first delta in the transcript while `running` is still true. | build |
| Q3 | AC2 | high | `anthropic_stream_assembles_like_a_whole_reply`: text, a `tool_use` in three `input_json_delta` fragments, and a thinking block with a signature. The assembled message equals the non-streamed fixture, and the tool runs with the parsed input. | build |
| Q4 | AC2 | medium | `anthropic_stream_keeps_usage_and_stop_reason`: `message_delta` usage and `stop_reason` reach the assembled message. | build |
| Q5 | AC3 | high | `openai_stream_assembles_split_tool_calls`: two tool calls whose `arguments` arrive in chunks by `index`, then `[DONE]`. Both run, and the result equals `from_chat` on the unstreamed fixture. | build |
| Q6 | AC4 | medium | `bad_streamed_input_is_a_tool_error`: a streamed input that is not JSON gives an error tool result, and the next turn runs. | build |
| Q7 | AC5 | high | `idle_stream_times_out`: with `RAM_CODE_IDLE_TIMEOUT=1`, a stream that pauses 3 s fails with an error that names the idle timeout. | build |
| Q8 | AC5 | medium | `active_long_stream_succeeds`: a stream that lasts longer than the old global timeout with an event every 500 ms succeeds. The test lowers the old limit through a test hook rather than waiting 120 s. | build |
| Q9 | AC6 | medium | `json_reply_to_a_stream_request_is_read_whole` | build |
| Q10 | AC6 | low | `streaming_can_be_turned_off`: with `"stream": false` on the provider, and with `RAM_CODE_STREAM=0`, the request body has no `stream` field. | build |
| Q11 | AC7 | high | `esc_stops_the_task_within_a_second`: Esc during a slow stream. Within 1 s `running` is false, the transcript shows `interrupted`, and the input takes text. | build |
| Q12 | AC8 | high | `interrupted_history_is_valid_for_both_formats`: an interrupt with one complete and one incomplete `tool_use`. The history keeps the partial text and the complete `tool_use` with an `interrupted by the user` result, and drops the incomplete block. The next request to each fake server passes its checks. | build |
| Q13 | AC9 | high | `interrupt_kills_the_command_group`: an interrupt during `sleep 60` in the sandbox. Within 1 s no process of that group runs. | build |
| Q14 | AC9 | high | `interrupt_cancels_the_run_wait`: an interrupt during `ramify run wait`. The fake `ramify` log has no `run go` and no `run block`. | build |
| Q15 | AC10 | medium | `stuck_worker_is_killed_and_history_rolls_back`: a fake worker that ignores `SIGINT` is killed after 3 s. The history equals the one before the task, and the transcript says so. | build |
| Q16 | AC11 | medium | `stream_error_keeps_partial_text`: an `error` event after two deltas. The partial text is in the transcript and the history, and the error shows. | build |
| Q17 | AC12 | medium | `headless_sigint_exits_130`: the test runs `ram-code --task` against the fake server and sends `SIGINT`. The process prints `interrupted` and exits with 130. | build |
| Q18 | AC13 | low | `max_tokens_defaults_per_provider`: `claude` gives 64,000; `deepseek`, `openai` and `env` give 8,192; a provider value and `RAM_CODE_MAX_TOKENS` override. | build |
| Q19 | AC14 | low | `esc_without_a_task_does_nothing`: Esc with no task changes no state. Ctrl+C sets `quit`. | build |
| Q20 | AC2, AC3 | medium | Read the diff: the SSE parser handles `ping`, multi-line `data:`, and events split across reads. No streamed text reaches the history twice. | audit |
| Q21 | AC1, AC7 | high | In Ram Code with a real provider, ask for a long answer. Text appears as it arrives. Press Esc mid-reply: `interrupted` shows within a second, and a follow-up message works. | qa |

## Write set

May change: `src/**`, `tests/**`, `README.md`, `.ramify/work/ram-code-streaming/**`.

Must not change: `Cargo.toml`, `Cargo.lock`, `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`, `.codex-plugin/**`, other `.ramify/work/**` folders.

If the build finds it needs a dependency, it stops and asks, and this plan adds `Cargo.toml` and `Cargo.lock` as a deviation.
