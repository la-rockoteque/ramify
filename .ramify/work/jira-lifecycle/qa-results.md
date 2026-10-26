# Jira lifecycle QA results

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `test_start_assigns_current_jira_user_and_moves_to_progress` and the CLI fixture verify the account and development status. |
| Q2 | pass | `test_open_request_enters_review_for_each_host`, host adapter tests, and the CLI fixture cover review. |
| Q3 | pass | `test_merged_request_enters_done_for_each_host` and host adapter tests use the host's merge state, without a Git ancestry check. |
| Q4 | pass | `test_watcher_detects_browser_changes_and_resumes_after_downtime` checks one-shot recovery. The CLI fixture changes remote state while the watcher runs. |
| Q5 | pass | `test_watcher_has_single_instance_and_stops_for_removed_worktree` verifies the watcher lock and missing-worktree exit. |
| Q6 | pass | `test_transition_mapping_rejects_missing_and_ambiguous_targets` and `test_transition_name_cache_handles_one_way_workflows` check mapping errors and repeated syncs. |
| Q7 | pass | `test_unconfigured_or_unlinked_branch_performs_no_writes`. All six Codex tests pass, including read-only resume without Jira. |
| Q8 | pass | `test_failed_sync_preserves_worktree_and_retries` checks authentication, timeout, rate-limit, and transition failures. |
| Q9 | pass | `test_repeated_sync_does_not_repeat_assignment_or_transition` and `test_partial_start_retries_without_reassigning` check complete and partial retries. |
| Q10 | pass | `test_declined_or_closed_unmerged_request_never_enters_done`. Host adapters distinguish closed and merged states. |
| Q11 | pass | `test_stale_request_never_reopens_done_story`. Jira rejects backwards updates from done before assignment or transition writes. |
| Q12 | pass | `test_host_lookup_matches_repository_and_source_branch`, `test_host_requests_over_http`, and `test_pagination` cover all three adapters. Cross-origin pagination is rejected. |
| Q13 | pass | The final smoke run passes all Jira closing and dry-run cases. The suite passes 187 cases with the two documented baseline colour failures. |
| Q14 | pass | README documents status mappings, all host credentials, polling, retries, logs, and disablement. The setup skill includes Jira settings. |
| Q15 | pass | `test_new_and_watcher_update_jira_without_manual_sync` starts a real worktree and local HTTP service. The watcher performs assignment, progress, review, and done writes once. |

Yield: 21 lifecycle tests and 6 Codex tests pass. Smoke passes 187 cases; two existing terminal-colour cases fail.

## Acceptance audit

AC1: `ticket_auto` starts synchronization after worktree creation. `jira.start` resolves the current Jira account and assigns it.
AC2: `request_state` supports GitHub, GitLab, and Bitbucket Cloud. An open request selects review.
AC3: The host's confirmed merged state selects done, including squash merges.
AC4: `watch` polls every 60 seconds by default. Session hooks resume it; `ticket sync` recovers missed changes.
AC5: `jira.transition` selects one exact status, transition name, or transition ID and reports invalid mappings.
AC6: Jira and ticket-key guards precede external API calls.
AC7: The state file preserves a pending start before API calls. API failures leave local work intact and the watcher retries.
AC8: Closed unmerged requests cause no status update. Jira leaves all done stories unchanged.
AC9: Existing closing commands retain their implementation. The legacy smoke fixture disables the new watcher; lifecycle fixtures test it separately.

The separate reviewer found two issues. GitLab now uses authenticated `glab api` when no token environment variable exists.
A delayed start now respects the latest request state instead of forcing a backwards development transition.
Three additional regression tests pass. The reviewer confirmed both fixes and found no remaining actionable issues.

The write-set check and `git diff --check` pass.
Shell syntax checks pass under the system Bash. Tests use asdf Python 3.12.15.

## Limits

Tests use local API fixtures. No real Jira story or hosted request was changed.
Bitbucket Data Center and other issue trackers are outside the approved scope.
Updates require a running local watcher and network access, or a later sync.
The previous work order documents two existing terminal-colour smoke failures on main.
