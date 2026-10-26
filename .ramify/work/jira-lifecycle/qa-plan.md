# Jira lifecycle QA plan

Use local HTTP fixtures and fake host clients. Tests must not change real stories or requests.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `test_start_assigns_current_jira_user_and_moves_to_progress` | build |
| Q2 | AC2 | high | `test_open_request_enters_review_for_each_host` | build |
| Q3 | AC3 | high | `test_merged_request_enters_done_for_each_host` including squash merges | build |
| Q4 | AC4 | high | `test_watcher_detects_browser_changes_and_resumes_after_downtime` | build |
| Q5 | AC4 | medium | `test_watcher_has_single_instance_and_stops_for_removed_worktree` | build |
| Q6 | AC5 | high | `test_transition_mapping_rejects_missing_and_ambiguous_targets` | build |
| Q7 | AC6 | high | `test_unconfigured_or_unlinked_branch_performs_no_writes` | build |
| Q8 | AC7 | high | `test_failed_sync_preserves_worktree_and_retries` for authentication, timeout, rate limit, and unavailable transition | build |
| Q9 | AC7 | high | `test_repeated_sync_does_not_repeat_assignment_or_transition` including partial success | build |
| Q10 | AC8 | high | `test_declined_or_closed_unmerged_request_never_enters_done` | build |
| Q11 | AC8 | high | `test_stale_request_never_reopens_done_story` | build |
| Q12 | AC2, AC3 | high | `test_host_lookup_matches_repository_and_source_branch` including unrelated requests and pagination | build |
| Q13 | AC9 | medium | Run existing Jira smoke cases and confirm dry runs perform no writes | qa |
| Q14 | AC1–AC9 | medium | Inspect README, setup guidance, host credentials, status mappings, polling interval, and retry instructions against implementation | audit |
| Q15 | AC1, AC4 | medium | Start a fixture-backed worktree, open and merge a fixture request, and confirm assignment and all three Jira stages | qa |

## Write set

Allowed paths:

```text
bin/ramify
libexec/jira.py
libexec/ticket*.py
libexec/ticket*.sh
hooks/*
.claude-plugin/plugin.json
skills/setup/*
skills/pr/*
skills/finale/*
skills/worktree/*
examples/*
test/*
README.md
.ramify/work/jira-lifecycle/*
```

Keep unrelated application screens and the codex-handoff work order unchanged.
