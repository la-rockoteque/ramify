---
name: apply
description: Workflow step `apply` — make the change scope.md describes, run the repo's checks, and commit as you go. Use when `ramify step` names apply as the next step.
---

# apply — make the scoped change

Follow the [workflow contract](../workflow/SKILL.md).

## Read first

`scope.md`, and the code or the config you are about to change.

## Do

1. Make each change `scope.md` lists, and nothing else.
2. Run the repo's checks that the change can break: build, type-check, lint, the tests near it.
3. Commit in small steps. Run `ocre-jelly` in `commit` mode on each message.

## Stop and ask when

- The change needs more than `scope.md` says. Update `scope.md` and ask the user again.
- The tweak turns out to change behaviour or to fix a bug. A tweak has one lane, so it cannot be
  raised. Tell the user, and start a feature or a bug work order with `ramify new` from the
  primary checkout. The user removes this worktree with `ramify delete <worktree>`.

## Prose

Comments say the non-obvious *why*, and nothing else. Run `ocre-jelly` in `docs` mode on the
comments you wrote.

## Record

When the change is made and the checks are green:

```bash
ramify step done apply
```
