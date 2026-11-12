# QA plan: Ram Code deny-by-default sandbox

The repo declares no services. The build settles the automated cases with `cargo test`. New cases go in `tests/qa_deny_sandbox.rs`. The #62 cases in `tests/qa_sandbox.rs` stay and must pass unchanged. Cases that run a real sandbox are `#[cfg(target_os = "macos")]`. A tool that is not installed skips its case with a printed reason. No case uses the network or opens a window.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `profile_starts_with_deny_default`: the generated profile's first rule after `(version 1)` is `(deny default)`, and it has no `(allow default)`. | build |
| Q2 | AC1 | high | `unlisted_operation_fails`: `ls /Users` in the sandbox fails with `Operation not permitted`. | build |
| Q3 | AC2 | high | `toolchains_work_in_the_sandbox`: in a fresh copy of a small Rust crate with an empty `target/`, `cargo build` and `cargo test --lib` succeed. | build |
| Q4 | AC2 | high | `c_build_script_works_in_the_sandbox`: a crate whose `build.rs` compiles a C file with `cc` builds in the sandbox. | build |
| Q5 | AC2 | high | `git_works_in_the_sandbox`: `git status`, `git log -1` and `git diff --stat HEAD~1` succeed in a test repo with two commits. | build |
| Q6 | AC2 | medium | `common_tools_work_in_the_sandbox`: `make --version`, `python3 -c 'print(1)'`, `node -e 'console.log(1)'`, `npm --version`, `grep`, `sed`, `awk` on a worktree file, and `sh`, `bash`, `zsh` running `echo`. | build |
| Q7 | AC3 | high | `reads_outside_the_roots_fail`: a file in a temp folder outside the allowed ones, a sibling repository, a fake `$HOME/.zshrc`, and `ls /Users` all fail. | build |
| Q8 | AC4 | high | The whole `qa_sandbox` suite: every case in `tests/qa_sandbox.rs` passes with no edit to that file. The audit checks the file is unchanged in the diff. | build |
| Q9 | AC5 | high | `cannot_signal_outside`: the test spawns `sleep 30` outside the sandbox. A sandboxed `kill -0 <pid>` fails, and the process still runs. | build |
| Q10 | AC6 | high | `linked_worktree_git_reads_common_dir`: in a linked worktree made with `git worktree add`, `git status` succeeds in the sandbox. Writing `info/exclude` in the common Git folder fails. | build |
| Q11 | AC7 | high | `sandbox_json_adds_paths`: a `read` path becomes readable and a `write` path writable. | build |
| Q12 | AC7 | high | `sandbox_json_cannot_unhide_secrets`: `~/.ssh` and a `.git` path in `sandbox.json` stay denied. | build |
| Q13 | AC7 | high | `project_sandbox_json_is_ignored`: a `sandbox.json` in the worktree has no effect. | build |
| Q14 | AC8 | medium | `invalid_sandbox_json_is_an_error`: invalid JSON makes `run_command` return an error that names the file, and a marker command does not run. | build |
| Q15 | AC9 | medium | `denied_command_gets_the_hint`: a denied read ends with the hint line, which names `sandbox.json` and `outside_sandbox`. A command that succeeds gets no hint. | build |
| Q16 | AC1–AC7 | high | Read the profile in the diff: every `allow` is narrow, each read root and write path has a reason, `process-exec` is not paired with a launch path, and the #62 denials still come after the allows. | audit |
| Q17 | AC10 | low | Read the README: it describes deny-by-default, the read roots, `sandbox.json`, the user temp and cache folders, and the hint. | audit |
| Q18 | AC2, AC9 | medium | In Ram Code on this repo, ask the model to run `cargo test --lib` and `cat ~/.zshrc`. The first passes. The second fails with the hint, and the model does not retry it outside the sandbox without asking. | qa |

## Write set

May change: `src/**`, `tests/**`, `README.md`, `.ramify/work/ram-code-deny-sandbox/**`.

Must not change: `Cargo.toml`, `Cargo.lock`, `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`, `.codex-plugin/**`, other `.ramify/work/**` folders.
