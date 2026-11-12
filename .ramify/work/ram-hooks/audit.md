# Audit: ram-hooks

Base: `feat/zsh-prompt` (#52). This branch is stacked on it. The tests are `tests/qa_hooks.rs`
(26), `cli/tests/hooks.rs` (6) and the unit tests in `cli/src/hooks.rs` (3).

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `src/agent.rs` `call_tool`: a deny returns `blocked by a hook: …` before `tools.call`. Tests `pre_tool_deny_blocks_the_command` and `pre_tool_ask_counts_as_deny`. |
| AC2 | met | `cli/src/hooks.rs` `merge`: exit 2 joins stderr into `deny`. Test `exit_two_blocks_with_stderr`. |
| AC3 | met | `HookSet::matching`: an anchored regex, with `*` and empty matching all. Test `matcher_selects_the_tool`. |
| AC4 | met | `call_tool` appends the PostToolUse contexts. Test `post_tool_context_reaches_the_model`. |
| AC5 | met | `run_turn_hooked` returns before the first request on a block. Test `prompt_block_sends_nothing`. |
| AC6 | met | The SessionStart and UserPromptSubmit contexts come before the task. Test `session_and_prompt_context_come_first`. |
| AC7 | met | A Stop block pushes the reason as a user turn, and `stop_hook_active` is true after. Test `stop_block_gives_one_more_turn`. |
| AC8 | met | `MAX_STOP_BLOCKS` is 3. Test `stop_blocks_at_most_three_times`. |
| AC9 | met | `finish` kills the hook at its timeout. Tests `a_hook_past_its_timeout_is_killed` and `a_failing_hook_does_not_stop_the_turn`. |
| AC10 | met | `matching` keeps one hook per (command, plugin root). Test `the_same_command_runs_once`. |
| AC11 | met | `HookSet::load` keeps the source order. The hooks of one event run in parallel, and a deny wins. Tests `every_source_runs_in_order`, `a_broken_source_is_skipped` and `a_deny_wins_over_an_allow`. |
| AC12 | met | `enabled_plugins` reads `enabledPlugins` and `installed_plugins.json`. Test `enabled_plugin_hooks_run_with_plugin_root`. |
| AC13 | met | `common` and each event's fields, with `RAM_CODE=1`. Test `hook_input_has_the_common_and_event_fields`. |
| AC14 | met | `Hooked::fire` returns before any process when no hook has the event. Test `no_hooks_no_process`. |
| AC15 | met | `hooks/ram.json` registers `pr-gate` on `run_command`. Test `ramify_pr_gate_denies_in_ram_code` runs the real hook, and `gh` never runs. |
| AC16 | partly met | `ramify_session_start_writes_the_ram_record` shows the record, the work order context, and its removal on SessionEnd, through `Hooked`. The interface path (`App::start_session` on a thread, the context file to the first worker, `end_session` at quit) has no automated test. QA case Q22 checks it live. |
| AC17 | met | README: "Hooks in Ram Code", with the sources, the events, the decisions and `hooks/ram.json`. |
| AC18 | met | `cli/src/classify.rs`. Test `classify_each_action`: each row, as `run_command` and as `Bash`, and 4 commands that do none. |
| AC19 | met | Test `classify_a_chain`. |
| AC20 | met | `target` skips the value of each flag that takes one. Test `classify_pr_details`. |
| AC21 | met | `call_tool` fires PreWorkflow per action, and a deny blocks the call. Test `pre_workflow_deny_blocks_the_merge`. |
| AC22 | met | Test `post_workflow_gets_details_and_output`. |
| AC23 | met | `ramify-hook workflow-pre` calls `ramify hooks bridge pre`. Test `bridge_denies_for_claude_code` runs the hook script with Claude Code's input. A live Claude Code session is QA case Q34. |
| AC24 | met | Ram Code: `Hooked::actions` returns before `ramify classify` without a workflow hook (test `no_workflow_hooks_no_classifier`, with a fake classifier that logs). Bridge: `bridge` returns before `workflow()` when `!set.has(event)` (read in the diff; test `bridge_without_workflow_hooks_says_nothing`). |
| AC25 | met | `file_action` uses the repo's `work_dir` too. Test `classify_a_work_item_read`. |
| AC26 | met | README: "Workflow events", the command table, `ramify classify` and the bridge. The review found that spec 23 was not met: nothing called `ramify hooks run`. The review removed it. |

Also covered: `continue: false` (`continue_false_ends_the_task`) and `systemMessage`
(`system_message_shows_in_the_transcript`).

## Audit cases

| Case | Verdict | Evidence |
|---|---|---|
| Q23 | pass | README as listed for AC17. |
| Q24 | pass | `run_turn` calls `run_turn_hooked` with `Hooked::none`, whose `fire` starts no process. SessionStart runs in `App::start_session` on a thread. `end_session` waits on a channel for at most 5 s. |
| Q33 | pass | README as listed for AC26. |

## Write set

`ramify write-set .ramify/work/ram-hooks/qa-plan.md`: the diff stays inside it, with the
deviations it lists (`tests/qa_plan.rs`, and the files from `feat/zsh-prompt`).

## Found during the build

- The spec now says that a plugin's `hooks/ram.json` wins over its Claude Code hooks, and that
  ramify's `ram.json` is skipped when the ramify plugin is loaded. Without that rule, ramify's
  SessionStart ran twice and wrote two different records.
- Added: `ramify hooks list` (a listing, not an editor) and `RAMIFY_HOOKS_HOME`.
- Hook scripts run with the user's `HOME`. A test that changes `HOME` breaks `python3` through
  asdf shims, so the tests pass their hooks home with `Hooked::load_from` or `RAMIFY_HOOKS_HOME`.
- `prompt_is_fast_with_twenty_worktrees` (from #52) failed once at 53.5 ms, under the load of
  the full parallel suite. It passed 3 times out of 3 alone. This branch does not touch the
  prompt path.
