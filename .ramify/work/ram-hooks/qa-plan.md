# QA plan: hooks in Ram Code

The automated cases go in a new integration test file, `tests/qa_hooks.rs`. It drives
`run_turn` with a fake model, as `tests/qa_plan.rs` does. Hook files and scripts live in a temp
folder, and `HOME` points there so the user's real `~/.ram` and `~/.claude` are never read.
Each hook script appends its stdin to a log, so a test can read back what each hook received.
The ramify cases (AC15, AC16) run the real `hooks/ramify-hook` on a fixture repo, as
`cli/tests/prompt.rs` does.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `pre_tool_deny_blocks_the_command`: the command would create a file; the file does not exist, and the tool result is `blocked by a hook: <reason>` | build |
| Q2 | AC1 | medium | `pre_tool_ask_counts_as_deny` | build |
| Q3 | AC2 | high | `exit_two_blocks_with_stderr` | build |
| Q4 | AC3 | high | `matcher_selects_the_tool`: a `write_file` hook does not run on `run_command`; `*` and an empty matcher run on both; `run_.*` runs on `run_command` only | build |
| Q5 | AC4 | medium | `post_tool_context_reaches_the_model` | build |
| Q6 | AC5 | high | `prompt_block_sends_nothing`: the fake model counts zero requests | build |
| Q7 | AC6 | medium | `session_and_prompt_context_come_first`: the first request's user message starts with the SessionStart context, then the UserPromptSubmit context, then the prompt | build |
| Q8 | AC7 | high | `stop_block_gives_one_more_turn` | build |
| Q9 | AC8 | high | `stop_blocks_at_most_three_times` | build |
| Q10 | AC9 | high | `a_hook_past_its_timeout_is_killed`: a 1 s timeout, a hook that sleeps 30 s; the turn ends in under 5 s, the tool ran, and one transcript event names the hook | build |
| Q11 | AC9, AC8 | medium | `a_failing_hook_does_not_stop_the_turn`: exit 1 gives one transcript line, and the tool runs | build |
| Q12 | AC10 | medium | `the_same_command_runs_once` | build |
| Q13 | AC11 | high | `every_source_runs_in_order`: one hook per source, each appends its source name; the log has all six, in source order | build |
| Q14 | AC11 | medium | `a_broken_source_is_skipped`: a source that is not JSON gives one transcript line, and the other sources run | build |
| Q15 | AC12 | high | `enabled_plugin_hooks_run_with_plugin_root`: one enabled and one disabled plugin; only the enabled one runs, and `CLAUDE_PLUGIN_ROOT` is its folder | build |
| Q16 | AC13 | high | `hook_input_has_the_common_and_event_fields`: for each event, the logged JSON has `session_id` `ram-<pid>`, `cwd` the worktree, `hook_event_name`, the event fields, and `RAM_CODE=1` in the environment | build |
| Q17 | AC11 | medium | `a_deny_wins_over_an_allow` | build |
| Q18 | AC14 | medium | `no_hooks_no_process`: with no source, the hook runner reports zero spawns for a turn with tool calls | build |
| Q19 | AC15 | high | `ramify_pr_gate_denies_in_ram_code`: the real `hooks/ram.json` and `ramify-hook` on a fixture with a work order missing steps; `gh pr create` through `run_command` is denied with the pr-gate reason, and no `gh` runs | build |
| Q20 | AC16 | high | `ramify_session_start_writes_the_ram_record`: SessionStart from `hooks/ram.json` writes `ram-<pid>.session` with agent `ram`, its context holds the work order note, and SessionEnd removes the record | build |
| Q21 | AC16 | medium | `ram_code_no_longer_writes_its_own_record`: with no hook source, a task writes no `ram-<pid>.session` | build |
| Q22 | AC1–AC16 | high | Live: in `~/dev/ramify-wt/ram-hooks`, run the branch's Ram Code with a work order that has steps missing. Ask it to run `gh pr create --title x --body y`. The transcript shows the pr-gate deny, and no PR opens. A shell prompt in the worktree shows `◆ <name>` while Ram Code runs. | qa |
| Q23 | AC17 | low | The README has the sources, events, decisions and `hooks/ram.json` | audit |
| Q24 | AC14 | medium | Read the diff: the tool loop calls no hook code when no hook is loaded, and the interface thread never waits on a hook (SessionStart in a thread, SessionEnd bounded at 5 s) | audit |
| Q25 | AC18 | high | `classify_each_action` in `cli/tests/hooks.rs`: one table row per tool call of spec 17, with the expected action; a plain `ls` gives `[]` | build |
| Q26 | AC19 | high | `classify_a_chain`: `cd wt && git commit -m x && git push origin HEAD` gives `commit` then `push`, with `branch: "HEAD"` | build |
| Q27 | AC20 | medium | `classify_pr_details`: `--draft` gives `draft: true`; `gh pr merge 50 --squash` gives `pr: 50` | build |
| Q28 | AC25 | medium | `classify_a_work_item_read`: `read_file` and Claude Code's `Read` of `.ramify/work/x/spec.md` give `workitem.read` with `work_item` | build |
| Q29 | AC21 | high | `pre_workflow_deny_blocks_the_merge` in `tests/qa_hooks.rs`: the command would create a file; it does not exist, and the result has the reason | build |
| Q30 | AC22 | high | `post_workflow_gets_details_and_output`: `ramify step done build` through `run_command`; the hook log has `details.step`, `succeeded` and the output | build |
| Q31 | AC23 | high | `bridge_denies_for_claude_code` in `cli/tests/hooks.rs`: `hooks/ramify-hook workflow-pre` with Claude Code's PreToolUse JSON for `gh pr create`, and a `pr.open` deny in `.ram/settings.json`, prints a PreToolUse deny | build |
| Q32 | AC24 | medium | `no_workflow_hooks_no_classifier`: in Ram Code and in the bridge, with no PreWorkflow or PostWorkflow hook, `ramify classify` never runs (a fake `ramify` logs each call) | build |
| Q33 | AC26 | low | The README documents the actions, the details, `ramify classify`, `ramify hooks run` and the bridge | audit |
| Q34 | AC21 | medium | Live: a PreWorkflow hook on `pr.merge` in `~/.ram/settings.json` that denies. Ask Ram Code to merge a PR: the transcript shows the deny. Ask a branch Claude Code session (`--plugin-dir`) to do the same: Claude Code shows the same deny. | qa |

## Write set

```
src/**
tests/qa_hooks.rs
tests/qa_prompt.rs
hooks/ram.json
hooks/ramify-hook
README.md
cli/src/**
cli/tests/hooks.rs
.claude-plugin/plugin.json
hooks/codex.json
Cargo.toml
Cargo.lock
.ramify/work/ram-hooks/**
tests/qa_plan.rs
.ramify/work/zsh-prompt/**
cli/tests/prompt.rs
shell/**
!cli/tests/parity.rs
!cli/tests/reference/**
!workflows.yml
!skills/**
```

Deviations, added during the build:

- `tests/qa_plan.rs`: Ram Code now runs ramify's hooks found next to its binary. The TUI test
  sets `RAMIFY_HOME` to its temp folder so it runs no hook.
- `.ramify/work/zsh-prompt/**`, `cli/tests/prompt.rs`, `shell/**`: these come from
  `feat/zsh-prompt` (#52), which this branch is stacked on. The check compares with
  `origin/main`, so they show until #52 merges.
