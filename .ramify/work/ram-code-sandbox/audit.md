# Audit: Ram Code sandbox

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `src/sandbox.rs` `seatbelt_profile` allows `file-write*` under the worktree and the task temp folder, by real path | `sandbox_allows_writes_in_the_worktree`, `sandbox_allows_the_task_temp_folder` |
| AC2 | met | `src/sandbox.rs` `seatbelt_profile` denies `file-write*` elsewhere. `bwrap` binds `/` read-only. | `sandbox_denies_writes_outside`, `sandbox_denies_writes_through_a_symlink` |
| AC3 | met | `(deny network*)`, local Unix sockets only. `bwrap --unshare-net`. | `sandbox_denies_the_network` |
| AC4 | met | `src/sandbox.rs` `scrub`, `is_secret` | `sandbox_strips_keys`, `scrub_list_covers_every_provider_key` |
| AC5 | met | `src/tools.rs` `run_with_timeout`, `timeout_of` | `timeout_kills_the_process_group`, `timeout_is_clamped`, `timeout_keeps_partial_output` |
| AC6 | met | `src/tools.rs` `ask_human`. `cli/src/run.rs` `--no-window`. | `outside_sandbox_asks_ramify_run`, `tui_asks_without_the_window`, `run_ask_no_window_skips_the_window` |
| AC7 | met | `src/app.rs` run event handling, `src/panel.rs` script view, `src/shell.rs` highlighter | `pending_run_opens_the_run_pane`, `run_pane_highlights_the_script` |
| AC8 | met | `src/tools.rs` `ask_human` returns `run wait`'s output | `blocked_run_reaches_the_model` |
| AC9 | met | `src/tools.rs`: `Sandbox::None` goes to `ask_human` | `no_sandbox_goes_to_the_human` |
| AC10 | met | `src/tools.rs` `ask_human` on a wait timeout | `no_decision_leaves_the_run_pending` |

## Q19, read the diff

The build profile met the rules the plan named, but it did not meet "no path skips the sandbox". Two gaps, both fixed in `b5e6a80`:

1. **Reads of credential files.** The profile started from `(allow default)` and allowed reads everywhere. `cat ~/.ssh/id_*` or `~/.aws/credentials` put a secret in the transcript, which goes to the provider. That defeats the aim of AC4. The profile now denies reads under 14 credential paths in `HOME`, by real path. `bwrap` hides them with a tmpfs or `/dev/null`. Test: `sandbox_hides_secret_files`. It failed on the build profile: `.ssh/id_test was readable: exit 0`.
2. **Launches outside the sandbox.** `open`, `osascript` and `launchctl` hand work to Launch Services or launchd, which start the program unconfined. The profile now denies Apple events, the `com.apple.coreservices.launchservicesd` lookup, and running those programs. Test: `sandbox_blocks_launching_outside`. It failed on the build profile: `osascript -e 'return 1' ran: exit 0`.

The escape through Launch Services was not run end to end, because that would open an app on the user's screen. The fix blocks every step it needs.

After the fix, `cargo test --lib` and `git log` still work inside the hardened profile. I checked by hand with `sandbox-exec -f`.

## Notes

- The build added local Unix sockets to the network allowance. TCP to `127.0.0.1` stays denied (Q5).
- `RAM_CODE_*` variables are scrubbed as well. That goes beyond the spec, and it is safe.
- `git commit` fails in the sandbox: a worktree's Git data lives in the main checkout, and allowing those writes would also allow edits to `.git/hooks`. The README says so.
- Writes to `~/.cargo` and `~/.npm` are denied, so a fetch or a registry lock needs `outside_sandbox`. An offline `cargo test` works.
- The profile is still allow-by-default with denials. A deny-by-default profile, as Codex uses, would be stricter, and is a candidate for a later branch.
- The Linux `bwrap` path is not tested on this machine.

## Checks

`cargo test` passes in every suite at the root and in `cli`. `cargo clippy --all-targets` reports no warnings. `ramify write-set .ramify/work/ram-code-sandbox/write-set.txt`: the diff stays inside the write set.
