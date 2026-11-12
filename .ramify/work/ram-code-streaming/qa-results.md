# QA results: Ram Code streaming and Esc

| Case | Result | Evidence |
|---|---|---|
| Q1 text_deltas_reach_the_events_file_before_finished | pass | `tests/qa_streaming.rs` runs the real worker against a fake SSE server; `text_delta` events precede `finished` in the events file. |
| Q2 transcript_shows_partial_text | pass | `tests/qa_streaming.rs`: `App::poll_events` shows the first delta before the fake worker finishes. |
| Q3 anthropic_stream_assembles_like_a_whole_reply | pass | `tests/qa_streaming.rs`: the streamed message equals the whole-reply fixture, thinking signature included. |
| Q4 anthropic_stream_keeps_usage_and_stop_reason | pass | `tests/qa_streaming.rs`: `usage` and `stop_reason` reach the assembled message. |
| Q5 openai_stream_assembles_split_tool_calls | pass | `tests/qa_streaming.rs`: split `arguments` by `index` assemble like `from_chat`; also `openai_name_split_across_fragments_with_id_first`. |
| Q6 bad_streamed_input_is_a_tool_error | pass | `tests/qa_streaming.rs`: an unparsable input gives an error tool result, and the loop continues. |
| Q7 idle_stream_times_out | pass | `tests/qa_streaming.rs`: a 3 s pause with a 1 s idle timeout fails and names the timeout. |
| Q8 active_long_stream_succeeds | pass | `tests/qa_streaming.rs`: a 2.5 s stream succeeds with `whole_timeout` at 1 s, and a whole reply slower than 1 s fails with the same setting. |
| Q9 json_reply_to_a_stream_request_is_read_whole | pass | `tests/qa_streaming.rs`: an `application/json` reply to a stream request is read whole. |
| Q10 streaming_can_be_turned_off | pass | `tests/qa_streaming.rs`: `"stream": false` and `RAM_CODE_STREAM=0` send no `stream` field. |
| Q11 esc_stops_the_task_within_a_second | pass | `tests/qa_streaming.rs` (fake worker), and `real_worker_handles_sigint_in_events_mode` (real worker). |
| Q12 interrupted_history_is_valid_for_both_formats | pass | `tests/qa_streaming.rs`: the saved history passes `anthropic_valid` and `chat_valid`. |
| Q13 interrupt_kills_the_command_group | pass | `tests/qa_streaming.rs` (macOS): no process of the command group survives the interrupt. |
| Q14 interrupt_cancels_the_run_wait | pass | `tests/qa_streaming.rs`: the fake `ramify` log has no `run go` or `run block`. |
| Q15 stuck_worker_is_killed_and_history_rolls_back | pass | `tests/qa_streaming.rs`, and `stuck_worker_kill_also_stops_hooks` for the processes the worker started. |
| Q16 stream_error_keeps_partial_text | pass | `tests/qa_streaming.rs` runs the real worker; the partial text is in the events and in the saved history. |
| Q17 headless_sigint_exits_130 | pass | `tests/qa_streaming.rs` runs `ram-code --task`; exit 130. `second_ctrl_c_kills_a_headless_run` covers a second Ctrl+C. |
| Q18 max_tokens_defaults_per_provider | pass | `tests/qa_streaming.rs`: per-provider defaults; `max_tokens_too_large_retries_with_the_model_limit` covers a model with a lower limit. |
| Q19 esc_without_a_task_does_nothing | pass | `tests/qa_streaming.rs`: Esc without a task changes nothing; Ctrl+C sets `quit`. |
| Q20 | pass after 2 fixes | `audit.md`: a cut OpenAI stream and a thinking-only stopped turn. Fixed in `cd3f90f`. |
| Q21 | pass | The human ran the case live and reported it passed in the chat on 2026-11-12. |

Yield: 21 cases. 21 ran and passed, 0 failed. The audit fixed 2 gaps.
