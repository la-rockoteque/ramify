# QA plan: zsh prompt segment

Automated cases go in a new integration test file, `cli/tests/prompt.rs`. It drives
`hooks/ramify-hook` with JSON on stdin, the built `ramify` binary, and `zsh -fc`. `test/smoke.sh`
calls the `libexec/` that `525feea` deleted, so it is not the place for new hook cases.
`ramify up` starts no QA stack for this repo. The `qa` cases run in the user's real terminal,
with oh-my-zsh and powerlevel10k.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `session_start_writes_a_claude_record`: the record has `agent: "claude"`, the root, a `pid` that runs and `state: "idle"` | build |
| Q2 | AC2 | medium | `session_start_with_codex_writes_a_codex_record` | build |
| Q3 | AC3 | high | `prompt_submit_and_stop_switch_the_state`: `user-prompt-submit` gives `working`, `stop` gives `idle`, and `since` does not change | build |
| Q4 | AC3 | medium | `state_change_without_a_record_writes_nothing`: `user-prompt-submit` for an unknown session creates no file | build |
| Q5 | AC4 | high | `session_end_removes_the_record`, for `logout`, `clear` and `other` | build |
| Q6 | AC4 | medium | `prompt_input_exit_keeps_the_record` | build |
| Q7 | AC5 | high | `a_dead_pid_is_skipped`: a record with a `pid` that does not run is not on the line | build |
| Q8 | AC5 | medium | `reap_removes_dead_session_records`, and keeps the live ones | build |
| Q9 | AC6 | medium | `no_record_without_a_ramify_config` | build |
| Q10 | AC7 | high | `prompt_matches_statusline_without_sessions`: in a linked worktree, `ramify prompt` prints the `ramify statusline` line for the same directory | build |
| Q11 | AC7 | medium | `prompt_in_the_primary_checkout`: `𖣂 main` | build |
| Q12 | AC8 | high | `two_sessions_in_since_order`: `· ✻ birch, ◇ ash ⏸` | build |
| Q13 | AC9 | high | `a_session_in_another_worktree_is_not_shown` | build |
| Q14 | AC10 | medium | `a_session_without_a_name_shows_its_agent`: `✻ claude`, `◇ codex` | build |
| Q15 | AC11 | medium | `prompt_is_silent_outside_git` and `prompt_is_silent_without_a_config`: no output, exit 0 | build |
| Q16 | AC11 | low | `prompt_survives_a_broken_record`: a file that is not JSON gives no session part and no error | build |
| Q17 | AC12 | medium | `prompt_json_has_statusline_fields_and_sessions` | build |
| Q18 | AC13 | high | `prompt_makes_no_http_request`: a service whose port does not answer, and the command finishes in less than the probe timeout. The diff has no call to `http_status_path` on the `prompt` path. | build, audit |
| Q19 | AC14 | high | `prompt_is_fast_with_twenty_worktrees`: median of 10 runs ≤ 50 ms on a fixture with 20 worktrees and a warm cache | build |
| Q20 | AC14 | medium | The health cache: `statusline` writes it. `prompt` reads it, and shows no health when it is older than 60 s | build |
| Q21 | AC15 | high | `zsh_plugin_prints_the_prompt_line`: `zsh -fc 'source shell/ramify.plugin.zsh; cd <worktree>; ramify_prompt_info'` | build |
| Q22 | AC15 | medium | `zsh_plugin_refreshes_on_cd`: after `cd` to another worktree, the next `precmd` gives the new line | build |
| Q23 | AC16 | high | In a terminal with p10k: add `ramify` to `POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS`, open a new shell. `cd` into a ramify worktree: the segment shows the line. `cd ~`: the segment is gone. | qa |
| Q24 | AC8 | high | Live: start Claude Code in a worktree, then Codex in the same worktree. The prompt in a third shell shows both names. Send a prompt to Claude: its `⏸` goes away while it works, and comes back when it stops. | qa |
| Q25 | AC17 | medium | `prompt_install_links_the_plugin`: with `ZSH_CUSTOM` in a temp dir, the link exists and `.zshrc` is not touched | build |
| Q26 | AC18 | low | The README has a section on `ramify prompt`, the session record, the plugin, the p10k segment and `ramify prompt install` | audit |
| Q27 | AC3 | medium | Both `.claude-plugin/plugin.json` and `hooks/codex.json` register `user-prompt-submit` | audit |

## Write set

```
cli/src/**
cli/tests/prompt.rs
hooks/ramify-hook
hooks/codex.json
.claude-plugin/plugin.json
.codex-plugin/**
shell/**
README.md
.ramify/work/zsh-prompt/**
!cli/tests/parity.rs
!cli/tests/reference/**
!workflows.yml
!skills/**
```
