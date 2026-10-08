---
name: review
description: Workflow step `review` — review the whole branch diff against the spec and the QA record, fix every finding, and get the user's OK. On the light lane it is also the conformance check. Use when `ramify step` names review as the next step.
---

# review — a senior read of the branch

Follow the [workflow contract](../workflow/SKILL.md). This step is a **human gate**.

## Get a second pair of eyes

Do not review your own work in the context that wrote it. Hand the diff to a separate reviewer:
a code-review agent or skill when one is installed, else a fresh subagent. Give it `spec.md`,
`qa-plan.md`, `qa-results.md` and the diff (`git diff $(git merge-base HEAD origin/main)`).
Ask it to attack the record, not to rebuild it.

## What it checks

- **Conformance**: every acceptance criterion is met, and the record's evidence holds up. On
  the light lane there was no audit, so this check carries the full weight.
- **Correctness**: edge cases, error paths, concurrency, data loss.
- **Security**: input at trust boundaries, permissions, secrets.
- **Simplicity**: code that already exists elsewhere, abstractions with one caller, dead code.
- **Tests**: does each test fail when the behaviour breaks?

## Do

1. Write the findings to `review.md`: severity, file and line, the failure scenario.
2. Fix every finding. Waive one only when the user accepts it explicitly, and write down why.
3. Run the checks again after the fixes.

## Prose

Run `ocre-jelly` on `review.md`, and in `docs` mode on comments the fixes added.

## Gate

Show the findings and the fixes, and ask for the OK. After the explicit OK:

```bash
ramify step done review --approved
```
