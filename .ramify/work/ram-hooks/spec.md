# Spec: hooks in Ram Code

## Problem

Claude Code runs the hooks a user or a plugin configures: commands that fire on session and tool
events, and that can block a tool call, add context for the model, or stop a turn. Ram Code has
no hooks. A gate, a guard or a context note that works in Claude Code does nothing in Ram Code.
This branch builds Claude Code's hook mechanics into Ram Code, and registers ramify's own hooks
for it.

## Behaviour

### Where the hooks come from

1. Ram Code reads hooks from these sources, in this order. All of them apply. A later source
   adds hooks and replaces none.
   1. `~/.ram/settings.json`
   2. `<project>/.ram/settings.json`, then `<project>/.ram/settings.local.json`
   3. `~/.claude/settings.json`
   4. `<project>/.claude/settings.json`, then `<project>/.claude/settings.local.json`
   5. The `hooks` of each Claude Code plugin that `~/.claude/settings.json` enables
      (`enabledPlugins`), from the plugin's `.claude-plugin/plugin.json` or `hooks/hooks.json`.
      `${CLAUDE_PLUGIN_ROOT}` is the plugin's folder.
   6. `<RAMIFY_HOME>/hooks/ram.json`: ramify's hooks for Ram Code (see 14).
2. `<project>` is the primary checkout of the project that Ram Code opened.
3. The format is Claude Code's: `{"hooks": {"<Event>": [{"matcher": "<regex>", "hooks":
   [{"type": "command", "command": "...", "timeout": <seconds>}]}]}}`. A hook of another type
   is skipped. A file that does not parse is skipped, and Ram Code says so once in the
   transcript.
4. One command that two sources register runs once per event.

### Events

5. Ram Code fires these events. The JSON on stdin has Claude Code's common fields:
   `session_id` (`ram-<pid>`), `transcript_path` (empty), `cwd` (the selected worktree) and
   `hook_event_name`, then the event's own fields.

   | Event | When | Own fields | Matcher tested against |
   |---|---|---|---|
   | SessionStart | Ram Code starts, and on `/clear` | `source`: `startup` or `clear` | `source` |
   | UserPromptSubmit | a message is sent, before the model sees it | `prompt` | — |
   | PreToolUse | before each tool call | `tool_name`, `tool_input` | `tool_name` |
   | PostToolUse | after each tool call | `tool_name`, `tool_input`, `tool_response` | `tool_name` |
   | Stop | the model has answered, with no tool call left | `stop_hook_active` | — |
   | SessionEnd | Ram Code quits | `reason`: `exit` | — |

6. `tool_name` is Ram Code's own: `run_command`, `write_file`, `read_file` or `list_files`.
   `tool_input` is the tool's input as the model sent it, for example `{"command": "git push"}`.
   A matcher is a regular expression on the whole name. An empty matcher or `*` matches every
   tool.
7. The hooks of an event run in parallel. Each one gets the JSON on stdin, runs in the selected
   worktree, and gets `CLAUDE_PROJECT_DIR` (the project), `RAM_CODE=1` and, for a plugin hook,
   `CLAUDE_PLUGIN_ROOT`. A hook past its `timeout` (60 s when unset) is killed, and that counts
   as a non-blocking error.

### Decisions

8. Exit 0: stdout is read as JSON when it starts with `{`.
   - `hookSpecificOutput.permissionDecision: "deny"` on PreToolUse: the tool does not run. The
     model gets `blocked by a hook: <permissionDecisionReason>` as the tool result.
   - `permissionDecision: "allow"` runs the tool. `"ask"` is handled as `"deny"`, with the
     reason, because Ram Code has no permission prompt.
   - `hookSpecificOutput.additionalContext`: the model gets the text. On SessionStart and
     UserPromptSubmit it comes before the user's message. On PostToolUse it is appended to the
     tool result.
   - `decision: "block"` with a `reason`:
     - On UserPromptSubmit, the message is not sent, and the reason shows in the transcript.
     - On PostToolUse, the reason is appended to the tool result.
     - On Stop, the model continues with the reason as a new user turn, and
       `stop_hook_active` is true on the next Stop. A Stop block repeats at most 3 times in a
       row.
   - `continue: false`: the task ends, and `stopReason` shows in the transcript.
   - `systemMessage` shows in the transcript.
   - Plain stdout that is not JSON is added as context on SessionStart and UserPromptSubmit,
     as in Claude Code, and ignored on the other events.
9. Exit 2: a blocking error. stderr is the reason. PreToolUse acts as a deny,
   UserPromptSubmit as a block, Stop as a block. On the other events it shows in the
   transcript.
10. Any other exit code: a non-blocking error. The first line of stderr shows in the transcript,
    and the event goes on.
11. When two hooks decide differently, a deny or a block wins over an allow. The reasons and
    the contexts are joined in the order of the sources.

### Where they run

12. The tool hooks run in the task's worker process, around each tool call. UserPromptSubmit
    and Stop run in the worker too. SessionStart runs in a background thread when Ram Code
    starts, and its context goes with the first task. SessionEnd runs when Ram Code quits, and
    Ram Code waits for it for at most 5 s. The interface never waits on a hook.
13. The transcript shows one line per hook that blocks, denies or fails: `hook <event>:
    <reason>`. A hook that only adds context shows nothing.

### Ramify's hooks for Ram Code

14. `hooks/ram.json` registers `hooks/ramify-hook` for Ram Code, with `RAMIFY_AGENT=ram` and
    Ram Code's tool names:
    - PreToolUse `run_command`: `behind`, `pr-gate`, `decide-guard`.
    - PreToolUse `*`: `human-gate`.
    - PostToolUse `run_command`: `push`, `worktree-add`, `auto-gate`.
    - SessionStart, UserPromptSubmit, Stop, SessionEnd: as in `hooks/codex.json`.

    The `allow` hook is left out: Ram Code has no permission prompt to skip.
15. `ramify-hook` treats `RAMIFY_AGENT=ram` as it treats Codex for the session title: it sets
    none. The session record from `feat/zsh-prompt` now comes from the SessionStart hook, with
    agent `ram`. `agent_pid` also stops at a `ram-code` process. Ram Code no longer writes the
    record itself, so the two paths cannot disagree.

### States

- No hook configured: Ram Code behaves as on `main`, with no extra process per tool call.
- A source that does not parse: skipped, with one transcript line.
- A hook that hangs: killed at its timeout, and the event goes on.

## Acceptance criteria

- AC1: Given a PreToolUse hook on `run_command` that prints a deny, when the model calls
  `run_command`, then the command does not run, and the tool result is
  `blocked by a hook: <reason>`.
- AC2: Given a PreToolUse hook that exits 2 with `nope` on stderr, then the tool does not run,
  and the result holds `nope`.
- AC3: Given a PreToolUse hook with matcher `write_file`, when the model calls `run_command`,
  then the hook does not run.
- AC4: Given a PostToolUse hook that returns `additionalContext`, then the tool result the model
  gets ends with that text.
- AC5: Given a UserPromptSubmit hook with `decision: "block"`, then the model gets no request,
  and the reason shows in the transcript.
- AC6: Given a UserPromptSubmit or SessionStart hook with `additionalContext`, then the model's
  first request contains the text before the user's message.
- AC7: Given a Stop hook that blocks once with `go on`, then the model gets one more turn with
  `go on`, and the next Stop has `stop_hook_active: true`.
- AC8: Given a Stop hook that always blocks, then the task ends after 3 blocks in a row.
- AC9: Given a hook that sleeps past its 1 s timeout, then it is killed, the tool runs, and the
  transcript shows one line.
- AC10: Given the same command in `~/.ram/settings.json` and `~/.claude/settings.json`, then it
  runs once per event.
- AC11: Given hooks in each source of 1, then all of them run, in the order of the sources.
- AC12: Given an enabled Claude Code plugin with hooks, then its hooks run, with
  `CLAUDE_PLUGIN_ROOT` set to its folder.
- AC13: Each hook gets the common fields and its event's fields on stdin, `cwd` is the selected
  worktree, and `RAM_CODE=1` is set.
- AC14: Given no hook in any source, then a tool call starts no extra process.
- AC15: Given ramify's `hooks/ram.json` and a work order with steps missing before `pr`, when the
  model runs `gh pr create` through `run_command`, then the call is denied with the pr-gate
  reason.
- AC16: Given ramify's hooks, when Ram Code starts in a configured worktree, then
  `sessions/ram-<pid>.session` has agent `ram`, the first task's request carries the work order
  note, and quitting removes the record.
- AC17: The README documents the sources, the events, the decisions and `hooks/ram.json`.

## Out of scope

- Hook types other than `command`, such as `prompt` hooks.
- Claude Code events that Ram Code has no equivalent for: Notification, PreCompact,
  SubagentStop.
- A permission prompt for `"ask"`.
- `/hooks` or any interface to edit hooks.
- Claude Code tool names (`Bash`, `Write`…) as aliases.

## Open questions

None. The user chose the sources (Ram's own first, then Claude Code's, then plugins) and Ram
Code's own tool names.

## Lane

Standard. It adds a new extension point that runs user commands, and changes how the session
record is written. This branch is stacked on `feat/zsh-prompt` and merges after it.
