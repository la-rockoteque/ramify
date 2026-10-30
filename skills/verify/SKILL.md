---
name: verify
description: Workflow step `verify` — build, type-check, lint and run the repo's whole suite on the branch; record each result in verify.md, and prove each failure main has too. Use when `ramify step` names verify as the next step.
---

# verify — the whole suite, not only the tests near the fix

Follow the [workflow contract](../workflow/SKILL.md).

## Read first

`diagnose.md`, the branch diff (`git diff $(git merge-base HEAD origin/main)`), and how the repo
defines its checks: the CI config, the package scripts, the Makefile, the README.

## Do

1. Run every check the repo defines: build, type-check, lint, and the whole test suite. Run
   them as CI runs them.
2. Fix each failure the branch causes, then run the checks again.
3. For each failure that remains, run the same check on a clean checkout of main
   (`git worktree add --detach <tmp> origin/main`). Remove that checkout after.
4. A failure that main has too is **known**. Write down its evidence: the command, the result
   on main and the result on the branch. Every other failure is the branch's to fix.

## Write `verify.md` next to the work order

| Check | Result | Evidence |
|---|---|---|
| The failing tests from `diagnose.md` | pass | `npm test`: 297 pass, the 3 new tests included |
| Lint | known | `npm run lint` fails on `src/old.ts` on main too, same error |

## Prose

Run `ocre-jelly` on `verify.md`.

## Record

When every check passes or is known, with evidence:

```bash
ramify step done verify
```
