---
name: pr
description: Workflow step `pr` — push the branch and open its pull request once every earlier step is recorded, with a body built from the work order. Use when `ramify step` names pr as the next step.
---

# pr — open the pull request

Follow the [workflow contract](../workflow/SKILL.md).

## Check the record

```bash
ramify step gate --pr      # exit 0: ready. Otherwise it names the missing steps.
```

The plugin refuses `gh pr create` until this passes. Do the missing steps. Do not route around
the gate.

## Write the body

Build it from the work order, not from memory:

- **Summary**: what changed and why, from `spec.md` (or `report.md`), a few bullets.
- **Acceptance**: each criterion and where it is met.
- **QA**: the Yield line from `qa-results.md`, and what a human still has to check.
- **Review**: the findings that were fixed or waived, from `review.md`.
- **Lane**: the lane, and why.

Run `ocre-jelly` on the body before you send it. Use the repo's PR template and attribution
lines when it has them.

## Open it

Push with upstream (`git push -u origin HEAD`), then `gh pr create` with the body. The push
hook may ask for a QA hand-over. Follow it when the repo has a QA stack.

## Record

```bash
ramify step done pr --note "#<number>"
```

Commit the work order with the step recorded, and push.
