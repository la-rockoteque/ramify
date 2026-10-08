---
name: audit
description: Workflow step `audit` — check the branch diff against the spec (or the bug report), criterion by criterion, settle the QA plan's audit cases with named evidence, and fill the gaps. Use when `ramify step` names audit as the next step.
---

# audit — does the diff do what was asked?

Follow the [workflow contract](../workflow/SKILL.md). This is the standard lane's conformance
check. The light lane skips it, and review carries it there.

## Read first

`spec.md` (or `report.md` for a bug), `qa-plan.md`, and the whole branch diff:
`git diff $(git merge-base HEAD origin/main)`.

## Do

For each acceptance criterion:

1. Find the code and the test that meet it. Name them: file, function, test name.
2. Verdict: **met**, **partly met** or **missing**.
3. Fix what is partly met or missing, with its test, before you go on.

Then settle each case the plan marks `audit` in `qa-results.md`:

| Case | Verdict | Evidence |
|---|---|---|
| Q4 | pass | `src/cart.ts` `applyDiscount`, test `discount caps at 100%` |

Also check the plan's write set (`ramify write-set qa-plan.md`), and look for criteria the
code meets by accident. A test that passes for the wrong reason is a gap.

## Prose

Run `ocre-jelly` on `qa-results.md`.

## Record

When every criterion is met and every `audit` case has evidence:

```bash
ramify step done audit
```
