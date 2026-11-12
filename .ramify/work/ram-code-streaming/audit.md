# Audit: Ram Code streaming and Esc

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `src/stream.rs` `post_stream`, `AnthropicAssembler`. `src/main.rs` writes text deltas. `src/app.rs` `read_worker_events`. | `text_deltas_reach_the_events_file_before_finished`, `transcript_shows_partial_text` |
| AC2 | met | `src/stream.rs` `AnthropicAssembler` | `anthropic_stream_assembles_like_a_whole_reply`, `anthropic_stream_keeps_usage_and_stop_reason`, `sse_parser_handles_crlf_comments_bare_data_and_split_utf8` |
| AC3 | met after a fix | `src/stream.rs` `OpenAiAssembler`, `src/openai.rs` `OpenAiLlm::stream` | `openai_stream_assembles_split_tool_calls`, `openai_name_split_across_fragments_with_id_first`, `openai_stream_cut_before_the_end_is_an_error` |
| AC4 | met | `src/stream.rs` `parse_input` → `INVALID_ARGUMENTS` | `bad_streamed_input_is_a_tool_error` |
| AC5 | met | `post_stream` idle check. `WHOLE_REPLY_TIMEOUT_SECS` applies only to the non-streamed `post_json`. | `idle_stream_times_out`, `active_long_stream_succeeds` |
| AC6 | met | `read_reply`: a reply that is not `text/event-stream` comes back whole | `json_reply_to_a_stream_request_is_read_whole`, `streaming_can_be_turned_off` |
| AC7 | met | `src/app.rs` Esc sends `SIGINT`. `src/cancel.rs`. | `esc_stops_the_task_within_a_second` |
| AC8 | met after a fix | `src/agent.rs` `close_turn`, `push_task` | `interrupted_history_is_valid_for_both_formats`, `an_interrupt_after_thinking_only_leaves_no_thinking_only_message` |
| AC9 | met | `src/tools.rs` cancel of the process group and of `ramify run wait` | `interrupt_kills_the_command_group`, `interrupt_cancels_the_run_wait` |
| AC10 | met | `src/app.rs` `STOP_GRACE`: the history file is used only after `finished` | `stuck_worker_is_killed_and_history_rolls_back` |
| AC11 | met | assembler `error`, `ReplyEnd::Failed` keeps the partial text | `stream_error_keeps_partial_text` |
| AC12 | met | `src/main.rs` headless `SIGINT`, exit 130 | `headless_sigint_exits_130` |
| AC13 | met | `src/providers.rs` default per provider | `max_tokens_defaults_per_provider` |
| AC14 | met | `src/app.rs` `on_key` | `esc_without_a_task_does_nothing` |

## Fixed during the audit (`cd3f90f`)

1. **A cut OpenAI stream passed as complete.** A stream that closed with no `finish_reason` and no `[DONE]` gave `ReplyEnd::Complete`, so a dropped connection gave a cut answer as if it were whole. The Anthropic path already failed such a stream without `message_stop`. It is now `ReplyEnd::Failed`, and the partial text stays. Test: `openai_stream_cut_before_the_end_is_an_error`. It failed before the fix (`Complete`).
2. **A thinking-only assistant message after a stop.** An interrupt after a finished thinking block, with no text or tool call yet, stored an assistant message that held only the thinking block. That is not a useful turn, and it risks a rejected next request. `close_turn` now keeps a stopped turn only when it has text or a tool call. Test: `an_interrupt_after_thinking_only_leaves_no_thinking_only_message`. It failed before the fix.

## Q20, read the diff

`SseParser` keeps bytes until a newline, so a UTF-8 character cut across reads stays whole. It strips `\r`, joins several `data:` lines with `\n`, accepts `data:` with no space, and skips `:` comments. `ping` and other events with no known type are ignored. Text goes into a block only through `text_delta`, so the history never holds it twice. The new parser test covers CRLF, a comment, `data:` with no space, two `data:` lines, and a character cut across reads.

Also checked, with no change needed:

- **Thinking blocks:** their text and signature are joined from deltas and kept byte for byte. An incomplete thinking or `tool_use` block is dropped after a stop. Only text survives cut.
- **Parallel OpenAI tool calls:** they are kept by index. An `id` sent only in the first fragment and a name split across fragments both assemble (new test).
- **An HTTP error on a stream request:** it is read whole and reported with its status.
- **Two user messages in a row:** `push_task` joins a task to a trailing user message, so they never follow each other. A string becomes a text block, and tool results stay first. `to_chat` turns that into `tool` messages, then one user text.
- **The events file:** `read_worker_events` reads from a saved offset. The poll is not quadratic.
- **The history file:** the worker writes it before `finished`. A worker killed before `finished` leaves the TUI on the history from before the task, so a half-written `history.json` is never read.
- **The reader thread after a stop:** it holds only a channel sender. It never writes the events file, and process exit does not wait for it.

## Residual risks, not fixed

- `SseParser` does not treat a bare `\r` as a line end. No provider in use sends that.
- On a model with preserved thinking, a stopped turn with a finished thinking block and cut text after it sends an edited turn back. The API then drops that thinking block or rejects the request. This was not tested live.

## Checks

`cargo test`: 251 passed, 0 failed. `cargo clippy --all-targets`: no warnings. `ramify write-set .ramify/work/ram-code-streaming/write-set.txt`: the diff stays inside the write set.
