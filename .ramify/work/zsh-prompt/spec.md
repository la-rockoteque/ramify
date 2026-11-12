# Spec: zsh prompt segment

## Problem

The user works in a zsh shell next to several Claude Code and Codex sessions. The shell prompt
does not show the ramify state of the current directory, or which agent sessions work in that
worktree. barbarbar, a Rust prompt segment for oh-my-zsh, did this job. Its source is lost.
This branch puts the segment into ramify.

## Behaviour

### Session records

1. The `session-start` hook writes `${RAMIFY_STATE_DIR:-/tmp/ramify}/sessions/<session_id>.session`.
   The file holds one JSON object: `agent` (`claude` or `codex`, from `RAMIFY_AGENT`), `root`
   (the session root), `pid` (the agent process), `state` and `since` (epoch seconds).
2. The `user-prompt-submit` hook sets `state` to `working`. The `stop` hook sets it to `idle`.
   A new session starts `idle`.
3. The `session-end` hook removes the file on every reason except `prompt_input_exit`.
4. A session whose `pid` no longer runs counts as gone. Readers skip it. `ramify reap` removes
   its file.
5. The hook writes the record in a configured repo only, under the rules of the name file.
   A session in a repo without a ramify config gets no record.

### `ramify prompt [--json]`

6. The command reads the current directory, not stdin. It prints one line for a zsh prompt.
7. The line is the `ramify statusline` line for that directory, then the live sessions of the
   same worktree:
   `𖣂 tm-127 ⎇ feat/tm-127 · 2/3 up · feature 3/8 → build · ✻ birch, ◇ ash ⏸`
   - `✻` marks a Claude Code session and `◇` a Codex session.
   - Each session shows its name. A session without a name shows its agent: `✻ claude`.
   - `⏸` follows an `idle` session.
   - Sessions sort by `since`, oldest first.
8. With no live session in the worktree, the line has no session part.
9. Outside a git repo, or in a repo without a ramify config, the command prints nothing and
   exits 0.
10. `--json` prints the `statusline --json` object, plus `sessions`: an array of `{name, agent,
    state, since}`.
11. The prompt must stay fast. The command does not probe services over HTTP. It takes the
    stack health from a cache that `statusline` and `card` write, and shows nothing for health
    when the cache is older than 60 s. One run takes at most 50 ms in a repo with 20 worktrees.

### zsh integration

12. `shell/ramify.plugin.zsh` is an oh-my-zsh plugin. It defines `ramify_prompt_info`, which
    prints the `ramify prompt` line. It caches the line per directory and refreshes it at
    each `precmd`.
13. `shell/ramify.p10k.zsh` defines `prompt_ramify` for powerlevel10k. It uses
    `ramify_prompt_info` and draws nothing when the line is empty.
14. `ramify prompt install` links the plugin into `${ZSH_CUSTOM:-~/.oh-my-zsh/custom}/plugins/ramify`
    and prints the two lines to add to `.zshrc` and `.p10k.zsh`. It does not edit those files.

### States

- Empty: no ramify config, or no git repo. No output.
- Loading: none. The command runs once per prompt and does not wait.
- Error: an unreadable session file or state directory gives no session part, and no error.
- Success: the line above.

## Acceptance criteria

- AC1: Given a configured repo, when a Claude Code session starts, then
  `sessions/<session_id>.session` holds `agent: "claude"`, the session root, a live `pid` and
  `state: "idle"`.
- AC2: Given `RAMIFY_AGENT=codex`, when a session starts, then the record holds `agent: "codex"`.
- AC3: Given a live record, when `user-prompt-submit` fires, then `state` is `working`. When
  `stop` fires, then `state` is `idle`.
- AC4: Given a session that ends with any reason except `prompt_input_exit`, then its record
  is gone.
- AC5: Given a record whose `pid` does not run, then `ramify prompt` skips it, and `ramify reap`
  deletes it.
- AC6: Given a repo without a ramify config, when a session starts, then no record is written.
- AC7: Given the shell in a linked worktree with no live session, then `ramify prompt` prints
  the same line as `ramify statusline` for that worktree, except the session name.
- AC8: Given two live sessions in the worktree, `birch` (Claude Code, working) and `ash` (Codex,
  idle), then the line ends with `· ✻ birch, ◇ ash ⏸`, in order of `since`.
- AC9: Given a live session in another worktree, then the line does not show it.
- AC10: Given a session without a name, then it shows as `✻ claude` or `◇ codex`.
- AC11: Given a directory outside git, or a repo without a ramify config, then `ramify prompt`
  prints nothing and exits 0.
- AC12: Given `--json`, then the output has every `statusline --json` field and a `sessions`
  array of `{name, agent, state, since}`.
- AC13: Given a worktree whose services do not answer, then `ramify prompt` makes no HTTP
  request.
- AC14: Given a repo with 20 worktrees and a warm cache, then `ramify prompt` runs in 50 ms or
  less.
- AC15: Given zsh with the plugin loaded, then `ramify_prompt_info` prints the `ramify prompt`
  line for the current directory.
- AC16: Given powerlevel10k with `ramify` in its prompt elements, then the segment shows the
  line, and shows nothing outside a configured repo.
- AC17: `ramify prompt install` creates the plugin link and edits neither `.zshrc` nor
  `.p10k.zsh`.
- AC18: The README documents `ramify prompt`, the session record, the plugin, the p10k segment
  and `ramify prompt install`.

## Out of scope

- bash, fish and starship prompts.
- A Codex TUI status line. Codex has no custom status line command.
- Edits to the user's `.zshrc` or `.p10k.zsh`. The user adds the lines.
- Session records in the desktop app.
- Recovery of the lost barbarbar code.

## Open questions

1. A session record needs a `user-prompt-submit` hook for `working`. Codex has the
   `UserPromptSubmit` event too. Is a new hook on each prompt acceptable, or is `idle` and
   live enough?
2. Are the marks `✻` (Claude Code), `◇` (Codex) and `⏸` (idle) right?

## Lane

Standard. The branch adds a session record that other tools read, a new hook event and a new
command. The record is an external contract.
