# Review: Ram Code MCP over stdio

A separate, security-minded reviewer agent read the branch diff and the record. It edited nothing. Every finding is fixed in `5dd9a11` and `943d606`, with a test that failed on the old code. One part of finding 5 is documented, with the reason.

| # | Severity | Failure | Fix | Test |
|---|---|---|---|---|
| 1 | high | An approval covered the entry, not the file it runs. After `u` (or `n`) on `node tools/mcp.js`, the model could edit `tools/mcp.js`, and the next task ran it with no prompt, outside the sandbox with `u`. | A project server can no longer run outside the sandbox: `u` is refused for it. A user server keeps `u`. A project approval also covers the repository files the server runs, including the `node_modules/.bin` program `npx` resolves. A change asks again. | `project_server_cannot_run_outside_the_sandbox`, `approval_covers_repo_files_the_server_runs` |
| 2 | medium | The stored approval could differ from the entry shown, if `.mcp.json` changed while the human read the prompt. | The prompt remembers the entry it showed. A different entry at the keypress stores nothing and is shown again. | `approval_records_what_the_human_saw` |
| 3 | medium | Project `env` could change what runs (`PATH`, `NODE_OPTIONS`, `npm_config_registry`…), and the prompt did not show the resolved program. | The prompt shows the program the command runs (`runs:`), and the approval covers it. A project entry cannot set `PATH`, `*PATH`, `NODE_OPTIONS`, `DYLD_*`, `LD_*`, `npm_config_*` and similar. | `prompt_shows_the_resolved_command_and_flags_risky_env` |
| 4 | medium | One bad schema made every request fail, built-in tools included, and a huge tool list rode on every request. | At most 128 tools per server, 2,000-character descriptions, and an object schema required. Each dropped tool gets a note. | `bad_tool_schemas_are_dropped_and_lists_capped` |
| 5 | low | A child that left the group survived the kill, and the reap came before the group kill. | The server's process tree is listed while it runs and killed before the reap, so a `setsid` child dies too. | `server_descendants_die_with_the_task` |
| 6 | low | A newline or a zero-width or bidi character could fake or hide a line in the prompt, and headless output printed raw escapes. | Names, commands, arguments, env and notes are escaped on one line in the prompt, `/mcp` and headless output. | `prompt_text_is_escaped` |
| 7 | low | Duplicate tool names got hash-suffixed aliases that a `PreToolUse` rule on the exact name missed. | Tools whose names repeat, or read the same once cleaned, are refused. | `duplicate_tool_names_are_refused` |
| 8 | low | No size cap on one server message, and shutdown stopped servers one after another. | At most 8 MiB per message. Servers stop together. | `huge_server_message_is_bounded`, `shutdown_stops_servers_in_parallel` |

## Documented, not fixed

On macOS, a process that a server starts and abandons with a double fork has already left the server's tree when Ram Code lists it, so it can outlive the task. Inside the sandbox, it has no network unless the server was approved with `n`. The README says so. On Linux, `bwrap --unshare-pid --die-with-parent` covers it.

## Checks after the fixes and the merge with `main` (#63)

- `cargo test`: 295 passed, 0 failed.
- `cargo clippy --all-targets`: no warnings.
- `ramify write-set .ramify/work/ram-code-mcp/write-set.txt`: the diff stays inside the write set.
