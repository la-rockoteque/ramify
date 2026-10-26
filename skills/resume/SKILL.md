---
name: resume
description: Continue an existing Ramify work order after switching between Claude Code and Codex, restarting a session, or compacting context. Use when asked to pick up work another agent left in a worktree.
---

# Resume an existing work order

Follow the [workflow contract](../workflow/SKILL.md), including its CLI resolution and runtime wait instructions.

## Recover the branch state

Open the existing worktree. If its path is unknown, use `git worktree list` and ask which branch to continue.
Stop the earlier agent's edits before continuing in that tree.
Run these read-only commands from the worktree:

```bash
pwd
git status --short --branch
git diff --stat
git diff
git diff --cached
ramify step
ramify step asked --json
```

Read the work order and the earlier step outputs beside it.
Inspect untracked files relevant to the task. Keep staged, unstaged, and untracked work intact.
Do not create a replacement branch or work order, reset files, or stash the earlier agent's edits to resume.
Do not require a chat transcript. Ask for missing intent only when the files do not establish it.

## Continue

If `ramify step` reports a repository workflow, follow its listed documents and native work item.
Use the repository's resume or orchestration instructions and preserve its existing step record.
Do not create a replacement Ramify work order or infer steps from a default lane.

- If a human gate is pending, use its existing request and wait for the human's decision.
- If the human already decided, read the recorded outcome and note before proceeding.
- If no gate is pending, continue the next incomplete step with its named skill.
- If the next step is partly implemented, compare the existing edits with the spec and QA plan, then complete it.
- If all steps are complete, report that state. Do not start another work order automatically.
- If there is no work order, report that result and ask whether to start one with the workflow skill.
- If the order is broken or a command fails, report the error and leave the step incomplete.

Before continuing a QA stack, use `ramify card` to inspect the existing state.
Use `ramify up` when the task needs the stack and it is down.
Do not stop or replace a running stack merely because the agent changed.

Use the worktree's absolute path as the working directory for every later command.
Record only steps you finish. Completed steps and pending approvals survive an agent switch.
