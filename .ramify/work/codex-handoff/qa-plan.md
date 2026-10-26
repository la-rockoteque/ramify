# QA plan: Codex handoff

## Cases

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | medium | Install the local Codex plugin in a fresh session. Confirm that its workflow skills load. | qa |
| Q2 | AC2, AC3 | high | Start from a Claude-created work order with unfinished edits. Open that worktree in Codex. Confirm that it reads the order, earlier outputs, Git status, and diff before edits. Confirm that it creates no branch or replacement order. | qa |
| Q3 | AC4 | high | Resume an order with completed spec and QA plan steps. Confirm that Codex selects build and preserves the completed record. | qa |
| Q4 | AC5 | high | Resume with a pending human gate. Confirm that the request remains pending until the human decides. | qa |
| Q5 | AC6 | medium | Read every shared skill. Confirm that commands resolve the installed CLI and waits use tools available in each runtime. | audit |
| Q6 | AC7 | high | Compare configuration, pending approvals, and stack reports from both agents in the same worktree. Confirm that they identify the same state. | qa |
| Q7 | AC8 | medium | Compare hook events, payloads, and responses against official Codex documentation. Verify that the README explains installation, trust, and manual lifecycle commands. | audit |
| Q8 | AC9 | high | Run the existing smoke suite, including Claude hook cases. All existing cases must pass. | build |
| Q9 | AC2, AC8 | medium | Open a worktree without an order. Confirm that the resume instructions identify the missing order without creating one silently. | qa |
| Q10 | AC5, AC6 | high | Reject a pending gate after handoff. Confirm that Codex reads the rejection and leaves the step incomplete. | qa |
| Q11 | AC1, AC8 | medium | Disable or leave Codex hooks untrusted. Follow the documented manual resume path and confirm that the work order remains readable. | qa |

## Automated coverage

Run `bash test/smoke.sh` for the existing behaviour.
The user selected both handoff states: unfinished build work and a pending approval.
The automated cases below exercise those states with Codex hook payloads.
The pending-approval case also verifies that a rejection and its note survive another resume.

| Case | AC | Risk | Test name | Settled by |
|---|---|---|---|---|
| Q12 | AC2, AC3, AC4 | high | `test_resume_preserves_unfinished_build` | build |
| Q13 | AC5 | high | `test_resume_preserves_pending_approval` | build |
| Q14 | AC1, AC8 | medium | `test_codex_package` | build |
| Q15 | AC2, AC8 | medium | `test_no_order` | build |
| Q16 | AC5, AC9 | high | `test_codex_decision_guard` | build |
| Q17 | AC6, AC8 | medium | `test_codex_worktree_output` (including compaction and session cleanup) | build |
A live Codex installation check requires a fresh session that loads this branch's plugin.
Report any unavailable live check as unverified.

## Write set

The branch may change these paths:

```text
.codex-plugin/*
.agents/plugins/*
hooks/*
skills/*
README.md
bin/ramify
libexec/app.sh
libexec/statusline.sh
test/*
.ramify/work/codex-handoff/*
```

Keep the existing Claude manifest and workflow definitions unchanged.
Change shared CLI code only where Codex integration or reliable approval-window startup requires it.

## Approval

Approve this plan after the handoff example identifies the input state and expected result.
