# QA results: Ram Code streaming and Esc

| Case | Result | Evidence |
|---|---|---|
| Q1 text_deltas_reach_the_events_file_before_finished | pass | `` |
| Q2 transcript_shows_partial_text | pass | `` |
| Q3 anthropic_stream_assembles_like_a_whole_reply | pass | `` |
| Q4 anthropic_stream_keeps_usage_and_stop_reason | pass | `` |
| Q5 openai_stream_assembles_split_tool_calls | pass | `` |
| Q6 bad_streamed_input_is_a_tool_error | pass | `` |
| Q7 idle_stream_times_out | pass | `` |
| Q8 active_long_stream_succeeds | pass | `` |
| Q9 json_reply_to_a_stream_request_is_read_whole | pass | `` |
| Q10 streaming_can_be_turned_off | pass | `` |
| Q11 esc_stops_the_task_within_a_second | pass | `` |
| Q12 interrupted_history_is_valid_for_both_formats | pass | `` |
| Q13 interrupt_kills_the_command_group | pass | `` |
| Q14 interrupt_cancels_the_run_wait | pass | `` |
| Q15 stuck_worker_is_killed_and_history_rolls_back | pass | `` |
| Q16 stream_error_keeps_partial_text | pass | `` |
| Q17 headless_sigint_exits_130 | pass | `` |
| Q18 max_tokens_defaults_per_provider | pass | `` |
| Q19 esc_without_a_task_does_nothing | pass | `` |
| Q20 | pass after 2 fixes | `audit.md`: a cut OpenAI stream and a thinking-only stopped turn. Fixed in `cd3f90f`. |
| Q21 | pass | The human ran the case live and reported it passed in the chat on 2026-11-12. |

Yield: 21 cases. 21 ran and passed, 0 failed. The audit fixed 2 gaps.
