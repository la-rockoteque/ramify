# Jira lifecycle

## Problem

Developers must assign stories and change their Jira status as work progresses.
Ramify should keep the story in step with the branch and its pull request or merge request.

## Behaviour

Use the Jira key in the branch name and the existing Jira credentials.
On a successful `ramify new`, assign the story to the authenticated Jira user and move it to In Progress.
When its pull request or merge request opens, move the story to In Review.
When the host confirms a merge, move the story to Done.
Support GitHub, GitLab, and Bitbucket Cloud.

Each project can configure the Jira status or transition for each stage.
Use an explicit review mapping because Jira has no review status category.
Preserve the existing done-category behaviour and `RAMIFY_JIRA_DONE` setting.
Resolve the current Jira user's account through Jira; Git host identity does not determine the assignee.

Check remote request state independently of command text so browser actions also count.
Provide a local watcher and a one-shot sync command.
The watcher starts after work starts and resumes with the session.
It checks at a documented interval while the machine and watcher run.
The next sync catches changes made while the watcher was offline.
Do not promise immediate updates while the machine is offline.

Without a ticket key or Jira configuration, leave the workflow unchanged.
Report missing credentials, invalid mappings, unavailable transitions, and network failures with a retry command.
Do not undo branch creation or interrupt development when synchronization fails.
Retain failed updates for the next sync.
Repeat syncs without duplicate assignments or transitions.
Never move a done story backwards because of a stale open request.
Use the configured repository and branch when matching a request.
A closed or declined request without a merge does not mark the story Done.

## Acceptance criteria

AC1. Starting work assigns the matching story to the authenticated Jira user and moves it to the configured development status.

AC2. An open request moves its story to the configured review status on GitHub, GitLab, and Bitbucket Cloud.

AC3. A confirmed merge moves its story to Done on all three hosts, including squash merges.

AC4. Browser actions synchronize within the polling interval while the watcher runs; one-shot sync recovers after downtime.

AC5. Project mappings select exact Jira statuses or transitions; invalid or ambiguous mappings produce an actionable error.

AC6. Missing configuration or a missing ticket key causes no external writes.

AC7. Authentication and network failures preserve local work and allow retry without duplicate updates.

AC8. Closed unmerged requests never finish a story; repeated or stale events never move a done story backwards.

AC9. Existing ticket closing, cleanup, completion, and dry runs keep their documented behaviour.

## Scope and lane

Use the standard lane because this feature changes external integrations and starts a background process.
Other issue trackers and Bitbucket Data Center require separate adapters.
Automatic changes after an unmerged request closes are outside this feature.
Draft requests follow the requested rule: an open request enters review.

## API references

- Jira: https://developer.atlassian.com/cloud/jira/platform/rest/v3/
- GitHub: https://docs.github.com/en/rest/pulls/pulls
- GitLab: https://docs.gitlab.com/api/merge_requests/
- Bitbucket Cloud: https://developer.atlassian.com/cloud/bitbucket/rest/api-group-pullrequests/
