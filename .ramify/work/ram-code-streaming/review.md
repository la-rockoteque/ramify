# Review: Ram Code streaming and Esc

A separate reviewer agent read the branch diff and the record. It edited nothing. Findings 1, 3 and 4–10 are fixed, and finding 2 is fixed in part. Each fix has a test that failed before it, unless the table says otherwise.

| # | Severity | Failure | Fix | Test |
|---|---|---|---|---|
| 1 | medium | A failed reply saved the task as a trailing user message, so a retry sent the task twice, and a request that always failed grew the history. | A reply that fails before any text or tool call restores the history from before the task. | `failed_reply_with_nothing_kept_restores_the_history` |
| 2 | medium | A stuck worker killed after 3 s left its hooks and their children running, and its temp folder stayed. | The kill finds every process of the worker (`pgrep -P`) and kills each group. The temp folder is removed. **Not done:** a hook wait does not check the cancel flag, because that needs `cli/src/hooks.rs`, outside the write set. A slow hook runs until the 3 s kill, which now ends it. | `stuck_worker_kill_also_stops_hooks` |
| 3 | medium | Esc before the provider resolved killed the worker with SIGINT's default action, and the transcript showed an error instead of `interrupted`. | `Cancel::on_sigint()` is the first line of `run_worker` and `run_headless`, and the SessionStart wait ends on a cancel. | `esc_before_the_provider_resolves_is_an_interrupt` |
| 4 | medium | No test sent SIGINT to the real worker, and Q8 passed for the wrong reason, through a production static. | A real-worker test, and a `whole_timeout` field in place of the static. Q8 also checks that a slow whole reply fails. | `real_worker_handles_sigint_in_events_mode` (passed first: it covers a path no test reached), `active_long_stream_succeeds` |
| 5 | low | A whitespace-only partial text block could get a 400. | Dropped, and `anthropic_valid` rejects it. | `whitespace_only_partial_text_is_dropped` |
| 6 | low | A second Ctrl+C could not stop a stuck headless run. | `SA_RESETHAND` and `SA_RESTART`. | `second_ctrl_c_kills_a_headless_run` |
| 7 | low | Four stream edge cases: `server_tool_use` input not parsed, a parser that was quadratic on a long line, OpenAI quiet after `finish_reason` counted as a failure, and `"error": null` counted as an error. | Each is fixed. | `server_tool_use_input_is_parsed`, `sse_parser_is_linear_on_a_long_line`, `openai_idle_after_finish_is_complete`, `openai_error_null_is_not_an_error` |
| 8 | low | `max_tokens` 64,000 failed on a model with a lower output limit. | On a "max_tokens too large" 400, one retry with the parsed limit (8,192 if none), kept for later tasks. | `max_tokens_too_large_retries_with_the_model_limit`, `max_tokens_limit_is_kept_for_the_session` |
| 9 | low | The TUI redrew every 50 ms, and a long streamed entry seemed to stop growing at 40 lines. | Redraw only on change, at most every 250 ms while running. A streaming entry shows its last 40 lines. | `redraw_only_when_something_changed`, `long_streaming_entry_shows_its_tail` |
| 10 | simplicity | Duplicated end mapping, a private copy of `Reply::whole`, and a thin `run_turn_hooked`. | Shared `reply_end` in `stream.rs`, `Reply::failed`, and `run_turn_hooked` removed. | the existing suite |
| T | risk | After Esc, a turn cut after a thinking block. | A stopped turn with no `tool_use` drops its thinking. A kept `tool_use` keeps it, as the API requires. | `stopped_turn_without_tool_use_drops_thinking` |

## Known limits

- A stream reader with a hard-coded timeout shorter than 2.5 s would still pass Q8. A test cannot rule that out without running for minutes.
- Hook waits ignore Esc (finding 2). A follow-up can add the cancel check in `cli/src/hooks.rs`.

## Checks after the fixes and the merge with `main` (#63)

- `cargo test`: 292 passed, 0 failed.
- `cargo clippy --all-targets`: no warnings.
- `ramify write-set .ramify/work/ram-code-streaming/write-set.txt`: the diff stays inside the write set.
