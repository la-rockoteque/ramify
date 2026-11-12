# QA plan: Ram Code sandbox

The repo declares no services. The build settles the automated cases with `cargo test`. Cases that run a real sandbox use `sandbox-exec` and run on macOS only (`#[cfg(target_os = "macos")]`). Linux CI skips them with a printed reason. Cases about `ramify run` use a fake `ramify` script that records its arguments and answers as `ask` and `wait` do. No test touches the real run registry.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `sandbox_allows_writes_in_the_worktree`: `echo hi > out.txt` succeeds, and the file holds `hi`. | build |
| Q2 | AC1 | medium | `sandbox_allows_the_task_temp_folder`: `touch "$TMPDIR/x"` succeeds. | build |
| Q3 | AC2 | high | `sandbox_denies_writes_outside`: writing to a sibling of the worktree fails, and the file does not exist afterwards. | build |
| Q4 | AC2 | high | `sandbox_denies_writes_through_a_symlink`: writing through a symlink in the worktree that points outside fails. | build |
| Q5 | AC3 | high | `sandbox_denies_the_network`: `curl` to a listener the test opens on `127.0.0.1` fails, and the listener sees no connection. | build |
| Q6 | AC4 | high | `sandbox_strips_keys`: with `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`, a custom `key_env`, `MY_SERVICE_TOKEN`, `A_SECRET` and `DB_PASSWORD_FILE` set, `env` shows none of them. `PATH` and `HOME` remain. | build |
| Q7 | AC4 | medium | `scrub_list_covers_every_provider_key`: a unit test of the scrub function over the names above and a few that must stay (`PATH`, `KEYBOARD`, `TOKENIZERS_PARALLELISM`). | build |
| Q8 | AC5 | high | `timeout_kills_the_process_group`: `timeout_seconds: 1` with `sleep 30 & sleep 30` returns in under 3 seconds with `timed out after 1 s`. No `sleep` from that group runs afterwards. | build |
| Q9 | AC5 | medium | `timeout_is_clamped`: `timeout_seconds: 5000` runs with 600. A missing value runs with 120. | build |
| Q10 | AC5 | low | `timeout_keeps_partial_output`: `echo start; sleep 30` returns `start` before the timeout note. | build |
| Q11 | AC6 | high | `outside_sandbox_asks_ramify_run`: with a fake `ramify`, the tool calls `run ask <script> --cwd <worktree> --note <reason>`. The script holds the command and the reason, and the tool returns what `run wait` printed (`ran: exit 0` and the output). | build |
| Q12 | AC6 | medium | `tui_asks_without_the_window`: from the TUI, `run ask` gets `--no-window`. From `--task`, it does not. | build |
| Q13 | AC6 | medium | `run_ask_no_window_skips_the_window`: in `cli/`, `ramify run ask --no-window` creates the run and does not call the window launcher. | build |
| Q14 | AC7 | high | `pending_run_opens_the_run_pane`: an `ask` event from the worker opens the panel on the Run pane and selects that run. The input hint says `waiting for your OK in the Run pane`. | build |
| Q15 | AC7 | high | `run_pane_highlights_the_script`: the rendered pane shows the script. A comment, a string and a variable each get their colour. `rm -rf`, `sudo`, `curl … \| sh` and `git push` are red. | build |
| Q16 | AC8 | high | `blocked_run_reaches_the_model`: a fake `run wait` that prints `blocked: too risky` makes the tool return `blocked: too risky`, and the command never runs. | build |
| Q17 | AC9 | high | `no_sandbox_goes_to_the_human`: with the sandbox program unavailable, a plain command goes through `run ask` and never runs unconfined. | build |
| Q18 | AC10 | medium | `no_decision_leaves_the_run_pending`: a fake `run wait` that times out gives the no-decision message, and no `run block` or `run go` is called. | build |
| Q19 | AC1–AC10 | medium | Read the diff: the profile denies `network*` and allows `file-write*` only under the worktree, the temp folder and the devices. The model cannot set the profile or the environment. No path skips the sandbox apart from `outside_sandbox` and AC9. | audit |
| Q20 | AC6–AC8 | high | In Ram Code, ask the model to run `curl -sI https://example.com`. It fails in the sandbox, then asks to go outside. The Run pane opens on a highlighted script with `curl` in red. Press `b` with a note: the model reports the block. Ask again and press `g`: the model gets the headers. | qa |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`, `cli/src/run.rs`, `cli/tests/**`, `.ramify/work/ram-code-sandbox/**`.

Must not change: `bin/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`, `.codex-plugin/**`, other `cli/src/**` files, other `.ramify/work/**` folders.
