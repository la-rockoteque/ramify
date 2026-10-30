---
name: fix
description: Workflow step `fix` — make the smallest change that makes diagnose's failing tests pass, with no refactor; commit as you go. Use when `ramify step` names fix as the next step.
---

# fix — the smallest change that makes the failing tests pass

Follow the [workflow contract](../workflow/SKILL.md).

## Read first

`diagnose.md`: the root cause, the failing tests and the fix plan. Then the code you are about
to change. Grep every caller of a function before you edit it.

## Do

1. Run the failing tests from `diagnose.md`. See them fail. When `diagnose` only named them,
   write them now, under those names, and see them fail.
2. Change the code at the root cause, as the plan says. Fix a shared problem in the shared
   function, not in each caller.
3. Run the failing tests again until they pass. Do not edit a test to make it pass.
4. Run the tests near the change. On the hotfix lane there is no `verify` step, so also run the
   repo's build, type-check and lint.
5. Commit in small steps. Run `ocre-jelly` in `commit` mode on each message.

Change nothing else: no refactor, no rename, no cleanup, no second bug. Write down what you see
for a later branch.

## Stop and ask when

- The plan does not work, or the cause is not the one `diagnose.md` names. Update
  `diagnose.md` and tell the user before you go on.
- The fix needs a migration, a new dependency or a change to a public contract. Raise the lane
  (`ramify lane standard`) when it does.

## Prose

Comments say the non-obvious *why*, and nothing else. Run `ocre-jelly` in `docs` mode on the
comments you wrote.

## Record

When every failing test from `diagnose.md` passes:

```bash
ramify step done fix
```
