# QA plan: Ram Code MCP (stdio)

This branch covers AC1–AC12, AC17 and AC18. AC13–AC16 belong to `feat/ram-code-mcp-oauth`. The repo declares no services. The build settles the automated cases with `cargo test`, in `tests/qa_mcp.rs`. The tests use a fake MCP stdio server that the test writes: a small Python 3 script speaking JSON-RPC over stdin and stdout. Its behaviour is set by arguments: tool list, pages, results, a hang, a crash, and a file where it writes its environment and its own pid. No case uses `npx`, `uvx` or the network. Cases that run a real sandbox are `#[cfg(target_os = "macos")]`. Approval prompts are driven through `App::on_key` with a fake runner, as in `tests/qa_plan.rs`.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `stdio_server_tools_are_offered_and_called`: a user `mcp.json` with the fake server. The tool list has `mcp__fake__echo`, and a call returns the server's text. | build |
| Q2 | AC2 | medium | `user_file_wins_over_project_file`: the same name in both files. The user entry's command runs. | build |
| Q3 | AC3 | medium | `config_variables_expand`: `${VAR}` and `${VAR:-default}` in `command`, `args`, `env`, `url` and `headers` expand. | build |
| Q4 | AC3 | medium | `unset_variable_fails_only_that_server`: a second server with an unset variable fails with an error that names it. The first server still works. | build |
| Q5 | AC4 | high | `project_server_waits_for_approval`: the fake server writes its pid when it starts. No pid file exists before the prompt is answered. | build |
| Q6 | AC4 | high | `denied_project_server_never_starts`: `d` at the prompt. No pid file, and no `mcp__` tool in the request. | build |
| Q7 | AC5 | high | `changed_entry_asks_again`: approve, then change one argument. The next task shows the prompt again. | build |
| Q8 | AC5 | medium | `headless_skips_unapproved_servers`: a headless run prints one line for the server and does not start it. | build |
| Q9 | AC5 | medium | `approval_is_stored_by_hash`: `mcp-approvals.json` holds the top level and a SHA-256 of the entry, and no `env` value. | build |
| Q10 | AC6 | high | `approved_server_is_sandboxed`: approved with `a`, the fake server's attempt to write outside the worktree and to open a TCP connection to a local listener both fail. | build |
| Q11 | AC6 | high | `network_approval_keeps_other_rules`: approved with `n`, the connection succeeds, and the outside write still fails. | build |
| Q12 | AC6 | medium | `npx_command_suggests_network`: the prompt for a `npx` command says why `n` may be needed. | build |
| Q13 | AC7 | high | `server_env_is_scrubbed`: with `OPENAI_API_KEY` set and `env: {TRACKER_TOKEN}`, the server's dumped environment has `TRACKER_TOKEN` and not `OPENAI_API_KEY`. | build |
| Q14 | AC8 | medium | `tools_list_follows_pages`: two pages give all tools. | build |
| Q15 | AC8 | medium | `tool_names_are_valid_and_unique`: a name with spaces, and two names over 64 characters that share a prefix, become valid, distinct names. | build |
| Q16 | AC9 | medium | `results_are_converted`: text, an image, a resource link and `isError: true` give the text, the placeholders and `error: …`. | build |
| Q17 | AC9 | low | `large_result_is_cut`: a 50,000-character result is cut at 40,000 with a note. | build |
| Q18 | AC10 | high | `hung_call_times_out`: with `timeout_seconds: 1`, a call that never answers returns an error in under 3 s, and the task goes on. | build |
| Q19 | AC11 | high | `crashed_server_restarts_once`: the first crash restarts the server; the second marks it failed. | build |
| Q20 | AC11 | high | `no_server_outlives_the_task`: after the task, no process with the recorded pids runs. | build |
| Q21 | AC12 | high | `pretooluse_hook_can_deny_an_mcp_call`: a hook denies `mcp__fake__delete`. The fake server's call log is empty, and the model gets the reason. | build |
| Q22 | AC17 | medium | `no_config_changes_nothing`: with no config, `/mcp` shows the empty state, and the tool list equals the built-in list. | build |
| Q23 | AC18 | medium | `both_formats_get_mcp_tools`: the Anthropic request and the Chat Completions request both list `mcp__fake__echo` with its schema. | build |
| Q24 | Scope | low | `http_entry_is_not_supported_yet`: an `http` entry shows `not supported yet` in `/mcp` and offers no tools. | build |
| Q25 | AC6, AC7 | high | Read the diff: every server start goes through the sandbox and the scrubber unless approved with `u`. No path starts a project server before approval. | audit |
| Q26 | AC1, AC4 | medium | In Ram Code with a real stdio server in this repo's `.mcp.json` (for example `@modelcontextprotocol/server-filesystem`), approve it with `n`, then ask the model to use one of its tools. The prompt shows the command, and the call works. | qa |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`, `.ramify/work/ram-code-mcp/**`.

Must not change: `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`, `.codex-plugin/**`, other `.ramify/work/**` folders.
