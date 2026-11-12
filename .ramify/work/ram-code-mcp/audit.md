# Audit: Ram Code MCP (stdio)

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `src/mcp.rs` `Session::start`, `Launch::start`, `ToolSource for Session` | `stdio_server_tools_are_offered_and_called` |
| AC2 | met | `src/mcp.rs` `load`: the user file first, a clashing project name skipped | `user_file_wins_over_project_file` |
| AC3 | met | `src/mcp.rs` `expand`, `read_transport` | `config_variables_expand`, `unset_variable_fails_only_that_server` |
| AC4 | met | `src/mcp.rs` `pending`, `Session::start` skips a server with no decision; `src/app.rs` prompt | `project_server_waits_for_approval`, `denied_project_server_never_starts` |
| AC5 | met after a fix | `src/mcp.rs` `parse_entry` hash, `Approvals` | `changed_entry_asks_again`, `headless_skips_unapproved_servers`, `approval_is_stored_by_hash`, `project_approval_binds_expanded_values` |
| AC6 | met after a fix | `src/mcp.rs` `Launch::command`; `src/sandbox.rs` `seatbelt_profile_with`, `bwrap_args_with` | `approved_server_is_sandboxed`, `network_approval_keeps_other_rules`, `npx_command_suggests_network`, `keychain_is_out_of_reach_for_servers` |
| AC7 | met after a fix | `src/mcp.rs` `base_env` from `scrub`, `expand_guarded` | `server_env_is_scrubbed`, `project_entry_cannot_read_secret_variables`, `user_entry_may_read_its_own_secrets` |
| AC8 | met | `src/mcp.rs` `list_all_tools`, `offered_name` | `tools_list_follows_pages`, `tool_names_are_valid_and_unique` |
| AC9 | met after a fix | `src/mcp.rs` `convert`, `visible` | `results_are_converted`, `large_result_is_cut`, `mcp_results_escape_control_characters` |
| AC10 | met | `src/mcp.rs` call timeout | `hung_call_times_out` |
| AC11 | met | `src/mcp.rs` restart once, `stop`, `kill_group` | `crashed_server_restarts_once`, `no_server_outlives_the_task` |
| AC12 | met | `src/agent.rs` `run_turn_with` runs the hooks around a `ToolSource` call | `pretooluse_hook_can_deny_an_mcp_call`, `hooks_see_mcp_tool_name_and_input` |
| AC17 | met | `src/mcp.rs` empty state | `no_config_changes_nothing` |
| AC18 | met | `ToolSource::definitions` joins both formats' tool lists | `both_formats_get_mcp_tools` |

## Gaps found and fixed

Each new test failed before its fix.

1. **A project file could pull secrets into a server.** `.mcp.json` comes with the repository. An entry such as `"env": {"X": "${ANTHROPIC_API_KEY}"}`, approved with `n` (which the prompt suggests for `npx`), gave a provider key to a server with network. The prompt showed only the env names. A secret in `args` was also printed, expanded, in the prompt. Now a project entry that expands a secret variable fails with a note to move the server to the user file. The rule is the scrubber's (`sandbox::is_secret`, now `pub(crate)`) plus the `key_env` names. User entries may still read secrets. Tests: `project_entry_cannot_read_secret_variables` (failed before), `user_entry_may_read_its_own_secrets` (guards the user case). Commit `1c5c00f`.
2. **An approval did not hold the expanded entry.** The hash covered the entry as written, so with `"args": ["${TARGET}"]` a new value of `TARGET` ran a different command with the old approval. A project hash now covers the entry and its expansion. Secrets cannot be part of it after fix 1. Test: `project_approval_binds_expanded_values` (failed before). Commit `1c5c00f`.
3. **The prompt hid `${VAR}` use.** It listed env names only. It now shows `NAME=value as written`. Test: `prompt_shows_env_as_written` (failed before). Commit `1c5c00f`.
4. **Control characters in results.** A server could return `ESC[2J`, BEL or C1 codes, which reach the model and the TUI transcript. `convert` now shows them as caret text (`^[`) or `\u{..}`. Test: `mcp_results_escape_control_characters` (failed before). Commit `1c5c00f`. The events file was safe already: the worker writes it with `serde_json`.
5. **The Keychain daemon was reachable.** This branch sits on `main`'s allow-by-default profile, which allows `com.apple.SecurityServer`. Both variants now deny the Keychain services. The test makes a throwaway keychain in the worktree with one dummy item, and never touches the user's keychains. Test: `keychain_is_out_of_reach_for_servers` (failed before, for both variants). Commit `a0b56d4`. `feat/ram-code-deny-sandbox` fixes the same hole in its profile; the merge must keep both.

## Q25, read the diff

- Every start goes through `Launch::command`. Only `Choice::Outside` (`u`) skips the sandbox. With no sandbox program, `a` and `n` fail with a note.
- `Session::start` starts only a server with a decision. The TUI only reads the files to show the prompt.
- `${VAR}` expansion reads the environment and runs nothing.
- The `n` variant allows IP traffic and the `mDNSResponder` socket only. Other Unix sockets stay denied, the Docker socket included (`network_approval_keeps_other_rules` and the by-hand check in the build). `.git` writes, credential reads, tty ioctl and launch escapes stay denied.
- Servers run with `setsid`, no terminal, and one process group that `stop` kills after a 2-second grace.

## Who protects the approvals file

On this branch, `~/.config/ram-code/mcp-approvals.json` sits outside the worktree and the task temp folder, so `main`'s profile denies writes to it. That holds for a sandboxed command and for a server approved with `a` or `n`. `write_file` and `edit_file` write only in the worktree. The file is still readable, which reveals approval choices only. `feat/ram-code-deny-sandbox` adds an explicit write denial on the config folder and stops reads outside its roots. A server approved with `u` can write the file, by the human's choice.

## Residual risks, for review

- A model can write `.mcp.json` with `write_file`. The next task then shows the prompt, and a human who approves without reading starts the server. The prompt shows the full command and env as written.
- Server tool descriptions reach the model unchanged, as in any MCP client.
- `run_command` and `read_file` output is not escaped for control characters. That is outside this branch.
- `pretooluse_hook_can_deny_an_mcp_call` and `hooks_see_mcp_tool_name_and_input` set process-wide variables. A lock keeps them apart.

## Checks

`cargo test`: 259 passed, 0 failed. `cargo clippy --all-targets`: no warnings. `ramify write-set .ramify/work/ram-code-mcp/write-set.txt`: the diff stays inside the write set.
