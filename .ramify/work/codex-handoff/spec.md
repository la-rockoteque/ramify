# Resume Ramify work in Codex

## Problem

A user starts work with Claude Code and later wants Codex to continue it.
Ramify stores workflow progress on disk, but its plugin and several skills assume Claude Code.

## Behaviour

The user installs Ramify for Codex and opens the existing worktree.
Codex reads the work order, prior step outputs, Git status, and the current diff before it edits files.
Codex continues the next incomplete step and preserves unfinished edits.
Both agents use the same work order, approval requests, configuration, and QA stack state.

With no work order, Codex reports that state and offers the normal workflow setup.
While a human decision is pending, Codex waits through the tools available in its runtime.
An error reports the failed action and leaves the workflow step incomplete.
After a successful step, either agent can read its recorded result and continue.

## Acceptance criteria

- AC1: Ramify provides a documented Codex plugin installation with access to its workflow skills.
- AC2: Codex can resume a Claude-created work order in the same worktree without creating another branch or work order.
- AC3: Resume instructions require Codex to inspect unfinished edits and earlier step outputs before it changes files.
- AC4: Completed steps remain complete, and Codex resumes the next incomplete step.
- AC5: A pending human gate remains pending after an agent switch. Codex cannot treat the switch as approval.
- AC6: Shared skills provide commands and wait instructions that work in both agents without requiring Claude-only environment variables.
- AC7: Both agents resolve the same existing configuration, approval requests, and stack state without a migration.
- AC8: Documentation identifies which Codex hooks are supported and gives explicit commands for any lifecycle actions without hook support.
- AC9: Existing Claude plugin behaviour continues to work.

## Out of scope

Import chat transcripts or private reasoning from Claude.
Run both agents as simultaneous editors in the same worktree.
Change the existing workflow lanes or human approval requirements.
Promise identical session titles or colours across agents.

## Open questions

None for the agreed same-worktree handoff.
Hook support will follow the documented Codex runtime contract.

## Lane

Standard. This change affects plugin installation, shared skills, and workflow continuation across agents.
