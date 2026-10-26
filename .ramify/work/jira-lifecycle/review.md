# Jira lifecycle review

The separate reviewer read the spec, QA plan, QA results, branch diff, and new implementation and test files.

## Findings and fixes

1. P1: GitLab CLI authentication used an unsupported `glab auth token` command.
   The adapter now uses authenticated `glab api --paginate --output ndjson` when no token environment variable exists.
   `test_gitlab_uses_authenticated_api_without_exporting_a_token` checks the command and parsed request state.

2. P2: A failed initial start could block a later merge in a workflow that cannot return to In Progress.
   Sync now checks the latest request state before replaying the pending start.
   An open or merged request skips the obsolete development transition while preserving assignment.
   `test_pending_start_does_not_block_a_later_merge` and `test_host_failure_does_not_block_initial_assignment` cover both paths.

The reviewer checked both fixes and ran the three new tests. No actionable findings remain.

## Validation

- 21 lifecycle tests pass, including live local HTTP fixtures and an automatic worktree watcher.
- All 6 Codex tests pass.
- Smoke passes 187 cases. The two terminal-colour failures match the baseline documented in the codex-handoff work order.
- The write-set check, whitespace check, and shell syntax checks pass.

No real Jira story or hosted request was changed.
Status updates require a running local watcher and network access, or a later manual sync.
The feature supports Jira Cloud, GitHub, GitLab, and Bitbucket Cloud.
