# Review: hooks in Ram Code

A fresh subagent reviewed `git diff $(git merge-base HEAD feat/zsh-prompt)..HEAD` against the
spec, the QA plan, the audit and the QA results. It ran the tests and probed the CLI with its
own inputs. It reported 21 findings: 3 high, 6 medium and 12 low. The fixes are below.

| # | Severity | Where | Failure | Fix | Status |
|---|---|---|---|---|---|
| 1 | high | `cli/src/hooks.rs` `finish` | A hook that leaves a child holding the pipe (`sleep 6 &`) ran 6 s past a 1 s timeout. | The output is read in chunks. After the hook exits, the readers get 200 ms at most. Test `a_hook_that_leaves_a_child_answers_in_time`. | fixed |
| 2 | high | `cli/src/hooks.rs` `start` | The input was written to stdin before the timeout started. A hook that does not read a 200 KB input blocked for 7 s and reported exit 0. | A thread writes stdin. The readers start at spawn. The deadline is taken at spawn. Test `a_hook_that_does_not_read_a_large_input_times_out`. | fixed |
| 3 | high | `HookSet::load`, the bridge | A cloned project ran its `.ram` and `.claude` hooks with no trust step. A model that writes `.ram/settings.json` got command execution through the bridge. | The user chose a trust list and a snapshot. The project's own files run only when `~/.ram/trusted.json` lists the project (`ramify hooks trust`). Ram Code reads the files once per session. The bridge reads a snapshot made at SessionStart. Tests `an_untrusted_project_runs_no_hook_of_its_own`, `a_task_waits_for_the_snapshot_of_session_start`, `bridge_runs_project_hooks_only_when_trusted` and `bridge_reads_the_hooks_of_session_start`. README warning added. | fixed |
| 4 | medium | `src/agent.rs` `call_tool` | `continue: false` on PreToolUse let the tool run. | The tool does not run, and the task ends. Test `pre_tool_continue_false_stops_before_the_tool`. | fixed |
| 5 | medium | `cli/src/hookcmd.rs` `hooks run` | The command tested the tool name for workflow events, and nothing called it. The audit said spec 23 was met. | `hooks run` is removed. Spec 23, AC26, the README and the audit are corrected. | fixed |
| 6 | medium | `cli/src/classify.rs` | `gh --repo=o/r pr merge`, `git --no-pager push`, `bash -lc`, `env -u`, `sudo` and `xargs` gave no action, so a PreWorkflow deny did not fire. | Wrappers and global flags are skipped by one function. 12 new rows in `classify_each_action`. | fixed |
| 7 | medium | `cli/src/classify.rs` | `git push -o ci.skip origin main` gave `branch: "origin"`, `--draft=true` gave `draft: false`, and `branch` had two meanings. | Flag values are skipped. `--draft=true` counts. The base of a new PR is `base`. Tests in `classify_pr_details`. | fixed |
| 8 | medium | `src/app.rs` | A first task sent before SessionStart ended lost the context. | SessionStart writes the snapshot, the context and a marker to a start folder. The task waits for the marker at most 10 s. Test `a_task_waits_for_the_snapshot_of_session_start`. | fixed |
| 9 | medium | `cli/src/hooks.rs` | A timeout killed `sh` only, and its children lived on. | Each hook runs in its own process group, and a timeout kills the group. `a_hook_past_its_timeout_is_killed` now checks that the child is dead. | fixed |
| 10 | low | `cli/src/hooks.rs` | Each timeout started when the hooks before it ended. | Fixed with 2. | fixed |
| 11 | low | the bridge | The bridge has 30 s, and its hooks had 60 s. Claude Code killed the bridge first, and the deny was lost. | The bridge caps each hook at 25 s. Documented. | fixed |
| 12 | low | `src/agent.rs` | The Stop limit counted per task, not in a row as spec 8 says. | A turn with a tool call starts the count again. Test `stop_blocks_count_in_a_row`. | fixed |
| 13 | low | `HookSet::matching` | A matcher on Stop, UserPromptSubmit or SessionEnd turned the hook off. | Events with no subject ignore matchers. Test `a_matcher_on_an_event_with_no_subject_is_ignored`. | fixed |
| 14 | low | `agent.rs`, `hookcmd.rs` | Two definitions of `succeeded`. | One function in `hooks.rs`. Null for a tool with no exit code, in the spec and the README. | fixed |
| 15 | low | `hooked.rs`, `hookcmd.rs`, `hooks.rs` | The merge code existed three times, and the workflow input twice. | `Outcome::absorb` and `workflow_fields` in `hooks.rs`. Ram Code and the bridge keep a loop of 4 lines each: one fires through `Hooked`, the other through `HookSet`. | fixed |
| 16 | low | `hooks.rs` | `Hook.env` was never set. | Removed. | fixed |
| 17 | low | `hooked.rs` | Ram Code and the CLI found ramify's home in two ways. | One `ramify_home` in `hooks.rs`, which needs both `bin/ramify` and `hooks/ram.json`. | fixed |
| 18 | low | `enabled_plugins` | The first install entry could belong to another project. | A user install, or a project install for this project. Test `a_plugin_installed_for_another_project_is_left_out`. | fixed |
| 19 | low | the bridge | Each Claude Code tool call ran `git rev-parse`, even with no workflow hook. | `git` runs only when a trust list exists. The bridge is a Claude Code hook, so `bash` and `ramify` still start on each call. | partly fixed |
| 20 | low | tests | `no_hooks_no_process` counted matched hooks, not processes. Some failure modes had no test. | `no_hooks_no_process` now logs each start of `sh` through a `PATH` shim. The new tests are listed above. | fixed |
| 21 | low | `src/app.rs` | A task that ends with no Stop (an error, the turn limit, a cancel) left the record `working`. | The interface sets the record to `idle` when a task ends. Test `a_task_that_ends_without_stop_leaves_the_record_idle`. | fixed |

Also found during the fixes: the bridge tests ran `hooks/ramify-hook`, which calls `bin/ramify`,
which prefers a release build. That build was older than the code, so the tests checked old
code. The bridge tests now run a copy of the hook whose `bin/ramify` is the binary under test.
The Ram Code tests build the release binary first.

Finding 19 stays partly fixed. A cheaper bridge would need Claude Code to run a binary with no
shell around it.

Checks after the fixes: `cargo test` passes in both crates (`tests/qa_hooks.rs` 36,
`cli/tests/hooks.rs` 8). `cargo clippy --all-targets` reports nothing in either crate.
