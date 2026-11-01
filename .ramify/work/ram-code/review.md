# Review: Ram Code

Independent review by a fresh subagent, read-only, against base `3a8a269`. It built the branch
and ran the suite. Every finding is fixed below unless a row says otherwise.

| # | Severity | Finding | Fix |
|---|---|---|---|
| 1 | high | `String::truncate` panicked when the limit fell inside a multibyte character, killing the worker. | `src/tools.rs` `truncate` walks down to a char boundary. Test `truncate_keeps_a_char_boundary`. |
| 2 | high | A symlink inside the worktree defeated `resolve` and let `read_file` and `write_file` escape. | `resolve` canonicalizes the nearest existing ancestor and rejects anything outside the canonical root. Tests `a_symlink_outside_the_root_is_rejected` and `a_parent_path_is_rejected`. |
| 3 | high | Quitting the TUI left the worker running and still editing the worktree. | `Drop for Worker` kills and waits. `Drop for Loader` does the same for the loader. |
| 4 | medium | An exit race reported a finished task as a failure and lost its history. | `poll_events` reads the events file again after `try_wait` reports the exit. The debug `eprintln` is gone. |
| 5 | medium | The empty state was unreachable for a repo with no config; `dash-project` exits 1. | `apply_dash_error` maps the `ramify setup` and `not inside a git checkout` messages to `Screen::Empty`. Q4 now uses that real failure. |
| 6 | medium | One malformed work order failed the whole project parse. | `RawWorkflow` is an untagged enum; a broken object renders as no workflow. |
| 7 | medium | Q3 and Q5 tested `App::invoke`, which no binary path calls; the worker protocol was untested. | Added `slash_action_runs_in_the_selected_root`, `worker_events_reach_the_transcript` and `async_load_lists_worktrees`. |
| 8 | medium | `run_command` gave the model the API token in the child environment. | `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL` and `RAM_CODE_MODEL` are removed from tool commands. |
| 9 | medium | Esc from the picker kept the new worktree and the old history. | `picker_from` restores the selection on Esc and Tab. |
| 10 | medium | The Loading screen never drew; the synchronous load froze the TUI. | `load_async` runs `dash-project` in a child; `poll_events` reads its output. Typed keys stay buffered while Loading. |
| 11 | low | Detail and Message showed keys they ignored. | Detail handles `u`, `d`, `s`, `q`; Message handles `q`; hints match. |
| 12 | low | The task folder was predictable and world-readable. | Unique name, mode `0700`, `create_new` for the history, folder removed on finish. |
| 13 | low | A task could not start with the letter `i`. | Inspect is `Ctrl+O`; the hint is updated. |
| 14 | low | A UTF-8 character split across two reads became U+FFFD forever. | The pending buffer is `Vec<u8>`; only complete lines are decoded. |
| 15 | low | Weak assertions: `contains("r")`, a vacuous Electron check, an ambiguous project name, a PTY race. | Q1 uses `poc-app`, Q4 checks `r retry`, the Electron string check is gone, and Q6 waits for the loader before quitting. |
| 16 | low | `.gitignore` lost `.omc/` and `__pycache__/`. | Both restored, with `target/` added. |
| 16b | low | `/new` always passed `--workflow feature`. | The action passes only the slug; ramify picks its default. |
| 17 | low | Dead code: `parse_dash`, `AgentEvent::Done`, the debug line. | Removed. `App::invoke` stays because the tests use it. |

## Checks after the fixes

- `cargo test`: 19 tests pass (7 unit, 12 QA).
- `cargo clippy --all-targets`: clean.
- `ramify write-set .ramify/work/ram-code/write-set.txt`: the diff stays inside it.
- Smoke: a chat task answers, `Ctrl+C` quits, and no `ram-code --task` process remains.

No finding is waived.
