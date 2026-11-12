# Review: Ram Code sandbox

A separate, security-minded reviewer agent read the branch diff and the record. It ran Seatbelt experiments on a copy of the profile, and it edited nothing. Every finding below is fixed, with a test that failed before its fix. One simplicity note stays, with the reason.

| # | Severity | Failure | Fix | Test |
|---|---|---|---|---|
| H1 | high | A sandboxed command could inject keystrokes into Ram Code's terminal with `TIOCSTI` on `/dev/tty`. A child that outlived the timeout could press `g` on a later pending run. Demonstrated. | Each command runs in its own session (`setsid`), with no controlling terminal. `/dev/tty` writes are no longer allowed, and `file-ioctl` on it is denied. | `sandbox_cannot_reach_the_terminal` |
| H2 | high | `(remote unix-socket)` reached the Docker socket, which is a full escape. Demonstrated against OrbStack's socket. | All `network*` is denied, Unix sockets included. That also closes `mDNSResponder` and the ssh-agent socket. | `sandbox_denies_unix_sockets` |
| H3 | high | A command could plant `core.fsmonitor` in Git config, which Git runs later outside the sandbox. Demonstrated. | Writes to any `.git` file or folder in the worktree are denied, nested ones included. | `sandbox_denies_git_metadata_writes` |
| H4 | high (Linux, reasoned) | `bwrap` saw host processes, so it could read Ram Code's environment, and the session bus at `/run/user/<uid>` was reachable. | `--unshare-pid`, `--unshare-ipc` and `--new-session`. `/run/user/<uid>` and the Docker sockets are hidden. | `bwrap_args_isolate_pids_and_sockets`, `bwrap_hides_the_user_runtime_and_docker_sockets` |
| M1 | medium | The Run pane showed only the first 40 lines, so line 41 could hide `curl … \| sh`. | The script view scrolls and wraps, and shows a red `N more lines below`. `g` works only after the last line has been on screen. | `long_script_shows_a_more_marker`, `long_script_cannot_run_unseen` |
| M2 | medium | A quote across two lines made real code look like a grey comment. Demonstrated. | Quote and heredoc state carry across lines. Risk stays red inside comments and strings. Control characters and bidi marks show as red escapes. | `highlighter_keeps_quote_state_across_lines`, `risk_is_never_greyed`, `hidden_characters_show_as_red_escapes` |
| M3 | medium | A child that escaped the group kill could hang the tool, and output memory was unbounded. | Non-blocking readers, 40,000 bytes kept at most, and reading stops 1 s after the command ends. | `escaped_child_cannot_hang_the_tool`, `huge_output_is_bounded` |
| M4 | medium | The scrubber missed `SSH_AUTH_SOCK`, `DOCKER_HOST`, URLs with passwords and more, and some credential files. | The names, name endings, URL values and secret paths are added. | `scrub_covers_review_names`, `secret_paths_cover_review_paths` |
| T1 | low | `sandbox_blocks_launching_outside` passed on any non-zero exit. | It requires exit 126 with `Operation not permitted`. | `sandbox_blocks_launching_outside` |

The reviewer found these sound: argument passing to `ramify run` (the reason cannot end the header comment), the frozen script with its sha256 check, quoting in the profile, and real-path matching.

## Kept

`runs_dir()` in `src/panel.rs` copies ramify's path logic. `ramify runs --json` does not give the folder, and the code that builds it is in `cli/src/commands.rs`, outside the write set. A comment says why.

## Residual risk, documented

No sandbox protects what the human runs later. A command can change a `Makefile`, `build.rs`, `.cargo/config.toml`, `.envrc`, `package.json` scripts or `node_modules/.bin`. The README tells the user to read the diff before running them. The profile is still allow-by-default with denials. A deny-by-default profile, as Codex uses, would be stricter, and is a candidate for a later branch.

## Checks after the fixes and the merge with `main`

- `cargo test`: 200 passed, 0 failed, at the root. `cli` tests: 98 passed.
- `cargo clippy --all-targets`: no warnings.
- The final profile was printed from the code and checked by hand under `sandbox-exec -f`: `cargo test --lib` passed, and `git status` and `git log` work.
- `ramify write-set .ramify/work/ram-code-sandbox/write-set.txt`: the diff stays inside the write set.
