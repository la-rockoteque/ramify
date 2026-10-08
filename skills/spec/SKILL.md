---
name: spec
description: Workflow step `spec` — write what a feature does and its acceptance criteria into the work order's spec.md, then get the user's OK. Use when `ramify step` names spec as the next step, or when asked to spec a feature on a branch with a work order.
---

# spec — what to build, and how we know it is done

Follow the [workflow contract](../workflow/SKILL.md). This step is a **human gate**.

## Read first

- The work order (`ramify step` prints its path) and what the user asked for.
- The code the feature touches. Trace the real flow end to end before you write a line.
- The repo's ADRs, domain glossary and conventions, when it has them. Name things with the
  glossary's terms. When the feature needs a term the glossary does not have, propose it to the
  user and add it to the glossary in this step.

## Write `spec.md` next to the work order

- **Problem**: who needs this and why, in two or three sentences.
- **Behaviour**: what the user sees or the system does, step by step. Name each state:
  empty, loading, error, success.
- **Acceptance criteria**: numbered, `AC1`, `AC2`… Each one is observable and testable alone.
  Write it as "Given … when … then …" or as one plain sentence with a measurable result.
- **Out of scope**: what this branch does not do, so review does not ask for it.
- **Open questions**: what only the user can decide. Ask them now. Do not guess.
- **Lane**: the lane, and one line of why. Say if the spec shows the lane must go up.

Keep the work order body to the summary: what and why, in a few lines, with a link to `spec.md`.

## Prose

Run `ocre-jelly` on `spec.md` and on the work order body before you show them.

## Gate

Show the spec and ask for the OK. Apply the changes the user asks for. After the explicit OK:

```bash
ramify step done spec --approved
```
