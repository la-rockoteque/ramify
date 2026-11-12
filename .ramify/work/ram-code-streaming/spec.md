# Ram Code streaming and interrupt

## Problem

Ram Code waits for each whole model reply. The transcript stays empty until the reply ends, and a long reply hits the 120 s request timeout. The user cannot stop a task that goes the wrong way, apart from quitting Ram Code. Claude Code and Codex stream text as it arrives, and Esc stops the work. This branch streams replies for both API formats and adds an Esc interrupt that keeps what finished.

The compaction and prompt-caching branch builds on the request path this branch changes. It merges after this one.

## Behaviour

### Streaming

Each model request sets `stream: true`.

- **Anthropic Messages**: Ram Code reads the server-sent events. It handles `message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`, `ping` and `error`.
  - `text_delta` adds text to the current text block.
  - `input_json_delta` adds a fragment to the current `tool_use` input.
  - `thinking_delta` and `signature_delta` build the current thinking block. A `redacted_thinking` block arrives whole in `content_block_start`.
  - `message_delta` gives `stop_reason`.
- **OpenAI Chat Completions**: Ram Code reads the `data:` lines up to `[DONE]`.
  - `choices[0].delta.content` adds text.
  - `choices[0].delta.tool_calls` adds fragments by `index`: the `id` and the function name arrive first, then pieces of `arguments`.
  - `finish_reason` gives the stop reason, as `from_chat` maps it today.

At the end of a stream, Ram Code builds the same message the non-streamed request returns today. The agent loop, the history, tool calls, hooks, `/provider` and `strip_thinking` see no difference.

- A `tool_use` input is complete only when its block ends. Ram Code parses it then, before any hook or tool runs. Input that does not parse is a tool error, through the `INVALID_ARGUMENTS` path.
- Thinking stays hidden in the transcript, as today.

### Showing text as it arrives

- The worker writes a `text_delta` event to the events file for each text fragment, and the closing `text` event as today.
- The TUI shows the growing text in the transcript. While a task runs, the main loop polls the events file every 50 ms, not every 120 ms.
- The events file stays the channel. A 50 ms poll of an appended file is fast enough for text, and it keeps the worker and the TUI as separate processes.

### Timeouts

- The 120 s timeout on the whole request goes away.
- A stream fails when no byte arrives for 90 s. Anthropic sends `ping` events during long thinking. `RAM_CODE_IDLE_TIMEOUT` sets the limit in seconds.
- Connecting fails after 30 s.

### `max_tokens`

- The `claude` preset defaults to 64,000, because a streamed reply has no request timeout.
- Every other provider keeps 8,192, because some compatible servers reject a larger value.
- A provider entry in `providers.json` may set `max_tokens`. `RAM_CODE_MAX_TOKENS` overrides every provider.

### A provider without streaming

- When the reply has the content type `application/json`, Ram Code reads it as a whole reply, as today.
- A provider entry with `"stream": false`, or `RAM_CODE_STREAM=0`, sends no `stream` field.

### Interrupt

Esc stops a running task. The panel takes Esc first when its own prompt is open, as today.

1. The TUI sends `SIGINT` to the worker. The transcript shows `stopping…`.
2. The worker stops reading the stream within 1 second. It also stops what is running:
   - A sandboxed `run_command`: Ram Code kills its process group.
   - A `ramify run wait`: Ram Code kills the wait. The run stays pending in the Run pane. Ram Code calls neither `run go` nor `run block`.
3. The worker writes the history, and keeps what finished:
   - Every completed turn.
   - The text the model wrote so far in the current reply.
   - A complete `tool_use` gets a `tool_result` with `is_error: true` and the text `interrupted by the user`. This applies to a tool that ran, a tool that was running, and a tool that never started.
   - An incomplete `tool_use` or thinking block is dropped, because its input or signature is missing. An assistant message left empty is dropped.
4. The worker writes an `interrupted` event, then `finished`. The transcript shows `interrupted`, and the input takes text again.

The next task's request must be valid for both formats. When the history ends with a user message of tool results, Ram Code adds the next task's text to that message as a text block, so two user messages never follow each other.

- **A stuck worker**: if the worker has not finished 3 seconds after `SIGINT`, the TUI kills it. The history rolls back to its state before the task, and the transcript says so.
- **Keys**: Esc with no running task does nothing, as today. Ctrl+C still quits Ram Code at once.
- **Headless runs**: in a `--task` run, `SIGINT` (Ctrl+C) does the same steps, prints `interrupted`, and exits with code 130.

### States

- **Streaming**: the reply grows in the transcript, and the input hint says `working… · esc stop`.
- **Stopping**: between Esc and the `finished` event.
- **Interrupted**: the transcript shows `interrupted`, and the input takes text.
- **Stream error mid-reply** (an `error` event, a dropped connection or an idle timeout): the partial text stays in the transcript and in the history, as on an interrupt, and the error shows. The task ends.
- **Success**: the same as today.

## Acceptance criteria

AC1. Given an Anthropic-format fake server that streams a reply in several `text_delta` events with pauses, when a task runs, then the worker writes `text_delta` events before `finished`. The TUI transcript shows part of the reply before the stream ends.

AC2. Given a streamed Anthropic reply with text, a `tool_use` whose input arrives in three `input_json_delta` fragments, and a thinking block with a signature, then the assembled message equals the non-streamed message for the same content. The tool runs with the parsed input.

AC3. Given an OpenAI-format fake server that streams text and two tool calls, with their `arguments` split across chunks by `index`, then Ram Code assembles both calls and runs them. The result equals what `from_chat` gives for the same reply unstreamed.

AC4. Given a streamed `tool_use` input that does not parse as JSON, then the tool result is an error, and the loop continues.

AC5. Given a stream that sends nothing for longer than `RAM_CODE_IDLE_TIMEOUT`, then the task ends with an error that names the idle timeout. Given a stream that lasts longer than 120 s with an event every few seconds, then it succeeds.

AC6. Given a server that answers a streamed request with an `application/json` reply, then Ram Code reads it as a whole reply. Given `"stream": false` on a provider, or `RAM_CODE_STREAM=0`, then the request has no `stream` field.

AC7. Given a running task, when the user presses Esc, then within 1 second the stream stops. The transcript shows `interrupted`, and the input takes text.

AC8. Given an interrupt in the middle of a reply that has one complete and one incomplete `tool_use`, then the history keeps the partial text and the complete `tool_use` with an `interrupted by the user` error result. It drops the incomplete block. A next task against Anthropic-format and OpenAI-format fake servers sends a request that the fake server's checks accept: valid roles, every `tool_use` matched by a `tool_result`, and no empty content.

AC9. Given an interrupt while `run_command` runs `sleep 60` in the sandbox, then the process group is gone within 1 second. Given an interrupt during a `ramify run wait`, then the wait stops, and the fake `ramify` gets no `run go` and no `run block`.

AC10. Given a worker that ignores `SIGINT`, then the TUI kills it after 3 seconds. The history rolls back to before the task, and the transcript says so.

AC11. Given an `error` event in the middle of a streamed reply, then the partial text stays in the transcript and the history, and the error shows.

AC12. Given a headless `--task` run, when it gets `SIGINT`, then it prints `interrupted` and exits with code 130. Its history follows AC8.

AC13. Given the `claude` preset, then `max_tokens` is 64,000. Given `deepseek`, `openai` or `env`, then it is 8,192. A provider's `max_tokens` and `RAM_CODE_MAX_TOKENS` override it.

AC14. Given no running task, Esc does nothing new. Ctrl+C quits Ram Code at any time.

## Out of scope

- Compaction and prompt caching, in their own branch, which builds on this request path.
- Showing thinking summaries or a tool's input while it streams.
- Resuming an interrupted task where it stopped. The next message starts a new turn with the kept history.
- Interrupts on Windows.

## Decisions

The user settled the open questions on 2026-11-12.

1. `max_tokens` defaults to 64,000 for `claude`. Other providers keep 8,192 unless their entry or `RAM_CODE_MAX_TOKENS` says otherwise.
2. A stream error offers no retry key. The user sends a new message.

## Lane

Standard. The branch changes the request path for every provider, and how the worker and the TUI stop each other.
