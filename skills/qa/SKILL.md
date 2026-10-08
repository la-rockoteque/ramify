---
name: qa
description: Workflow step `qa` — run the QA plan on the branch's live stack, reconcile the build and audit cases, fix what fails, and write qa-results.md. Use when `ramify step` names qa as the next step.
---

# qa — run the plan on the running app

Follow the [workflow contract](../workflow/SKILL.md).

## Start the stack

```bash
ramify up && ramify card
```

`card` prints this worktree's URLs. Use them, not the ports of another branch.

## Do

1. **Reconcile** the cases settled by `build` and `audit`: each named test exists, runs and
   passes. Each audit case has its evidence. Mark it in `qa-results.md`.
2. **Run** each case settled by `qa`, on the live stack: drive the browser or call the API as
   the plan says. Compare with the expected result.
3. **Fix** what fails, with a test where one can catch it, then run the case again.
4. When the plan has manual cases for a human, hand over with the `ask-for-qa` skill.

`qa-results.md` gets one row per case (Case, Result, Evidence), then a **Yield** line: how many
cases ran, passed, failed and were fixed. Keep screenshots out of the repo. Write "(screenshot,
local only)" in the evidence instead.

## Prose

Run `ocre-jelly` on `qa-results.md`.

## Record

When every case passes:

```bash
ramify step done qa
```
