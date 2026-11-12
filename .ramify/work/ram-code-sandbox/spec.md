# Ram Code sandbox

## Problem

Ram Code's `run_command` runs any shell command the model writes, with the user's full rights, no timeout, and the user's environment. The model can delete files outside the worktree, reach the network, or hang the task. It can also read `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY` and any custom provider key with `env`, and send them to the provider in its transcript. The system prompt tells it not to ask for permission. This branch confines the shell, adds timeouts, and sends anything that must leave the sandbox to the human through `ramify run`.

## Behaviour

### Sandboxed commands

`run_command` runs each command in a sandbox by default:

- **Reads**: allowed anywhere.
- **Writes**: allowed only in the worktree, a temp folder for this task, and `/dev/null` and the terminal devices.
- **Network**: denied.
- **Environment**: provider keys and tokens are removed. Removed are every `ANTHROPIC_*` variable, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`, each `key_env` named in `providers.json`, and any variable whose name ends in `_KEY`, `_TOKEN` or `_SECRET` or contains `PASSWORD`. `TMPDIR` points at the task's temp folder.
- **Mechanism**: on macOS, `sandbox-exec` with a profile Ram Code writes for the task. On Linux, `bwrap` when it is installed.
- **No sandbox**: when no sandbox program is available, a command does not run unconfined. It goes to the human as described below.

### Timeouts

- `run_command` takes `timeout_seconds`. The default is 120, and the most is 600.
- On a timeout, Ram Code kills the command's whole process group. The model gets the output so far and `timed out after N s`.

### Commands outside the sandbox

The model asks for this with `outside_sandbox: true` and a `reason`, for example to fetch dependencies or to write outside the worktree. The system prompt explains when to ask and says the human decides.

1. Ram Code writes the command to a script with the reason in a header comment.
2. It calls `ramify run ask <script> --cwd <worktree> --note <reason>`, then `ramify run wait <id>`.
3. The TUI opens the side panel on the Run pane and selects the new run. The transcript says the command waits for an OK in the Run pane.
4. The Run pane shows the selected run's script with shell highlighting: comments, strings, variables, keywords. Risky parts are marked in red: `rm -rf`, `sudo`, a pipe into `sh` or `bash`, `curl` and `wget`, writes to an absolute path, `git push`, and `chmod` or `chown`.
5. `g` runs it, and `b` blocks it with a note, as the Run pane does today.
6. The model gets `ran: exit N` with the output, or `blocked: <note>`.
7. The wait lasts at most 30 minutes (`RAM_CODE_RUN_WAIT` changes it). After that, the model gets `no decision; the run stays pending in the Run pane`.

In a headless run (`--task`), the same request goes through `ramify run`, and the desktop run window shows it.

States:

- Waiting: the run is pending, the Run pane shows it, and the input hint says `waiting for your OK in the Run pane`.
- Error: the sandbox profile fails, `ramify` is missing, or the run cannot be created. The model gets the error, and the command does not run.
- Success: the output goes back to the model.

## Acceptance criteria

AC1. Given a sandboxed command that writes a file in the worktree, then it succeeds.

AC2. Given a sandboxed command that writes to a path outside the worktree and the task's temp folder, then it fails, and the file does not exist afterwards.

AC3. Given a sandboxed command that opens a network connection, then it fails.

AC4. Given `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`, a custom `key_env` and `MY_SERVICE_TOKEN` in Ram Code's environment, then `env` inside a sandboxed command shows none of them. `PATH` and `HOME` remain.

AC5. Given `timeout_seconds: 1` and `sleep 30`, then the tool returns in about 1 second with `timed out after 1 s`. No process from the command is left running.

AC6. Given `outside_sandbox: true` and a reason, then Ram Code creates a pending run through `ramify run ask` with that note and the command in the script. It returns the result of `ramify run wait`.

AC7. Given a pending run from this session, then the TUI opens the Run pane on it, and the pane shows the script with highlighting. The risky parts listed above are marked.

AC8. Given `b` with a note on that run, then the model gets `blocked: <note>`, and the command never runs.

AC9. Given no sandbox program on the machine, then a command without `outside_sandbox` also goes to `ramify run` and does not run unconfined.

AC10. Given no decision within the wait limit, then the model gets the no-decision message, and the run stays pending.

## Out of scope

- Allow lists of commands that skip the sandbox, or network allow lists by domain.
- Windows.
- Gating `write_file` with the same approval.
- Gating commands through `PreToolUse` hooks. The hooks keep working as they do.

## Open questions

1. `ramify run ask` always opens the desktop run window. In the TUI, the Run pane is enough. Should this branch add `--no-window` to `ramify run ask` in `cli/src/run.rs`, and have Ram Code use it in the TUI? This spec says yes. It changes the ramify CLI, outside Ram Code.
2. Removing every variable that ends in `_KEY`, `_TOKEN` or `_SECRET`, or contains `PASSWORD`, may break a build that needs one, such as `NPM_TOKEN` or `CARGO_REGISTRY_TOKEN`. Is that acceptable? Such a build would then need `outside_sandbox`.
3. On Linux without `bwrap`, every command goes to the human. Is that acceptable, or should Ram Code refuse to start a task there?

## Lane

Standard, tag `permissions`. The branch changes what the model may do on the user's machine.
