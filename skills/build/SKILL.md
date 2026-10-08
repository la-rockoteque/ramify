---
name: build
description: Workflow step `build` — implement the spec and create every test the QA plan names, under that name, until they pass; commit as you go. Use when `ramify step` names build as the next step.
---

# build — the code and its tests

Follow the [workflow contract](../workflow/SKILL.md).

## Read first

`spec.md`, `qa-plan.md`, and the code you are about to change. Grep every caller of a function
before you edit it.

## Do

1. For each case settled by `build`, write its test first, under the name the plan gives. Run it
   and see it fail.
2. Write the smallest change that makes it pass. Reuse what the repo already has before you
   write something new. Fix a shared problem in the shared function, not in each caller.
3. Run the repo's checks: build, type-check, lint, the tests near the change.
4. Commit in small steps. Run `ocre-jelly` in `commit` mode on each message.
5. Keep the diff inside the plan's write set: `ramify write-set qa-plan.md`.

The QA stack runs from this worktree (`ramify up`, `ramify card`). Use it to see the change.

## Stop and ask when

- A criterion cannot be met as written, or two of them conflict.
- The change needs more than the spec says: a migration, a new dependency, a public contract.
  Raise the lane (`ramify lane standard`) when it does.

## Prose

Comments say the non-obvious *why*, and nothing else. Run `ocre-jelly` in `docs` mode on the
comments you wrote.

## Record

When every `build` case passes and the checks are green:

```bash
ramify step done build
```
