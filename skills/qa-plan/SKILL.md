---
name: qa-plan
description: Workflow step `qa-plan` — before any code, plan how each acceptance criterion is checked, tag each case with the step that settles it, and get the user's OK. Use when `ramify step` names qa-plan as the next step.
---

# qa-plan — the acceptance contract, written before the build

Follow the [workflow contract](../workflow/SKILL.md). This step is a **human gate**. Write no
code in this step: a plan written after the code tests what was built, not what was asked.

## Read first

`spec.md`, the work order, and the repo's test setup (unit, integration, end to end, and the
QA stack `ramify up` starts).

## Write `qa-plan.md` next to the work order

One row per case, at least one case per acceptance criterion:

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `test name` or the manual steps | build |

- **Risk**: high, medium or low. Put more cases on the high-risk criteria. Cover the edges:
  empty input, the error path, permissions, the largest realistic input.
- **Check**: for an automated case, the exact test name the build must create. For a manual
  case, the steps and the expected result.
- **Settled by**: `build` (a test the build writes), `audit` (read in the diff, standard lane
  only), or `qa` (run on the live stack).
- **Write set**: list the path globs the branch may change, and the ones it must not.
  `ramify write-set qa-plan.md` checks the diff against them later.

On the light lane there is no audit step, so no case is settled by `audit`.

## Prose

Run `ocre-jelly` on `qa-plan.md` before you show it.

## Gate

Show the plan and ask for the OK. After the explicit OK:

```bash
ramify step done qa-plan --approved
```
