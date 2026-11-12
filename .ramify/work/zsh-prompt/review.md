# Review: zsh-prompt

A separate reviewer agent read the diff against the spec and its Ram Code amendment, the QA
plan, the audit and the QA results. Plain and `--json` `statusline` output is byte-identical to
`origin/main` on a fixture with 1 of 2 services answering. The reviewer verified that.

## Findings and fixes

| # | Severity | Where | Failure scenario | Status |
|---|---|---|---|---|
| 1 | medium | `hooks/ramify-hook` `on_worktree_add`; `src/panel.rs` `sessions` | A session moved by `ramify new` kept the old `root` in its record. The prompt showed it in the old worktree, and the Sessions pane showed the old worktree too, where `main` showed the pinned one. | Fixed. The worktree-add hook moves the record's `root` with the pin. Test `a_move_to_a_new_worktree_moves_the_record`. |
| 2 | medium | `cli/src/prompt.rs` `install` | Verified. With a wrong `RAMIFY_HOME`, it printed "linked" and left a dangling link. It replaced any other `ramify` link without asking. | Fixed. It refuses a home without `shell/ramify.plugin.zsh`. It replaces a link that points elsewhere only with `--force`. It warns when the source is a plugin cache or a worktree, which can go away. Tests `prompt_install_refuses_a_home_without_the_plugin` and `prompt_install_keeps_a_foreign_link_without_force`. |
| 3 | medium | `hooks/ramify-hook` `write_record` | Verified. A compaction or a resume during a turn set the state to `idle`, so a working session showed `⏸`. | Fixed. Only `source: startup` resets the state. Test `compact_and_resume_keep_the_state`. |
| 4 | medium | `cli/src/statusline.rs` `render` | Verified. Without a fresh cache, `prompt --json` reported `"stack": "stopped"` while the stack may run. | Fixed. In prompt mode, `stack` is null without a fresh cache. Test `prompt_json_has_no_stack_without_a_fresh_cache`. |
| 5 | medium | `hooks/ramify-hook` `on_session_end` | `prompt_input_exit` kept the record. Once macOS reused the pid, a dead session showed as live, and reap never removed it. | Fixed. **This changes AC4**: SessionEnd now removes the record on every reason. The spec, the plan (Q6), the audit and the README say so. Test `prompt_input_exit_removes_the_record`. A crashed session can still meet a reused pid before `reap` runs. See waiver W1. |
| 6 | low | `shell/ramify.plugin.zsh` | Verified. A branch with `%F{red}` in its name turned a non-p10k prompt red. | Fixed. `ramify_prompt_info` escapes `%`, and the p10k segment no longer escapes it twice. Test `zsh_plugin_escapes_percent_for_any_theme`. |
| 7 | low | `cli/src/statusline.rs` `records`, `src/panel.rs` `sessions` | Verified. A name file with `\e[31m` printed a raw escape character. | Fixed. Both readers drop control characters. Test `a_control_character_in_a_record_is_dropped`. |
| 8 | low | `shell/ramify.plugin.zsh` | With an unbuilt `bin/ramify`, the first prompt ran `cargo build` and hung for minutes. | Fixed. The plugin skips a `ramify` whose Rust binary is missing. Test `zsh_plugin_skips_an_unbuilt_ramify`. |
| 9 | low | `src/app.rs` `refresh_status`, `refresh_health` | `ramify prompt` (about 45 ms in moship) ran on the draw thread, and each selection change could start one more health thread. | Fixed. Both run on a background thread, one run at a time. A new selection clears the old line at once. `header_shows_the_prompt_line` now also checks that the other worktree's line never shows. |
| 10 | low | `src/panel.rs` `running` | Pid 0 counted as live, because `kill -0 0` targets the process group. Each refresh forked one `kill` per record. | Fixed. `libc::kill`, with pid 0 and below counted as dead. The root crate now depends on `libc`. |
| 11 | low | `hooks/ramify-hook` `on_session_end` | A session id of `../x` removed `<state>/x.session`. | Fixed. The same id guard as the writers. Test `session_end_ignores_a_session_id_with_a_path`. |
| 12 | low | `tests/qa_plan.rs` | `worker_events_reach_the_transcript` wrote its record to the real `/tmp/ramify`. | Fixed. The test sets its own `RAMIFY_STATE_DIR`. |
| 13 | low | `hooks/ramify-hook` | Both writers used one `<path>.tmp`, so concurrent hooks raced, and a late `set_state` could bring back a removed record. | Fixed. One writer, `record_set`, with a temp file per process. `--present` drops a write when the record is gone. Test `a_late_state_change_does_not_bring_back_a_record` (it passes on the old code too, because it cannot reproduce the race; it guards the behaviour). |

## Waivers proposed (need your OK)

- **W1:** a crashed session's record can meet a reused pid before `reap` runs. The fix stores
  the process start time and compares it, which costs one `ps` per record on every prompt.
  Proposed: accept the gap. `reap` runs at each session start.
- **W2:** the marks and labels, the liveness check and the record parsing exist in both crates:
  `cli/src/statusline.rs` and `src/panel.rs`. They are separate crates with no shared library.
  Proposed: leave it until the crates share code. `feat/ram-hooks` plans a shared module.
- **W3:** `session_start_writes_a_claude_record` proves that the pid is live, not that it is the
  agent's. Under `cargo test` there is no agent process to find. QA case Q24 covers it live.

## Tests that passed for the wrong reason, now tightened

- `sessions_pane_lists_live_records` checks `◇ codex ⏸`, and checks that `✻ birch` has no `⏸`.
- `prompt_json_has_statusline_fields_and_sessions` was blind to the stale `stack`; the new test
  covers that.

## Checks after the fixes

- `cd cli && cargo test`: 36 tests in `cli/tests/prompt.rs` pass, and every other CLI test too.
- `cargo test` at the root: every test passes, `tests/qa_prompt.rs` (4) included.
- With the fixes taken out, 10 of the 11 new tests fail.
- `cargo clippy --all-targets`: no warnings, in both crates.
