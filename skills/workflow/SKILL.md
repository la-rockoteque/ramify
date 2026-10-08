---
name: workflow
description: The contract every ramify workflow step follows — read the branch's work order, do the next step with its skill, record it, respect human gates, raise the lane when the work turns out heavier, and pass all prose through ocre-jelly. Use when a branch has a work order (`ramify step` prints one), when asked to start or continue a feature, bug, tweak or spike, or when a step skill points here.
argument-hint: "[start <workflow> <slug> | next]"
---

# Run a work order

A **work order** is `.ramify/work/<slug>/order.md`: YAML frontmatter (`workflow`, `lane`,
`branch`, `tags`, `created`, `steps`), then the prose that says what the work is and why. The
step outputs (`spec.md`, `qa-plan.md`, `qa-results.md`, `review.md`) sit next to it. It is
committed with the branch.

## Start one

```bash
ramify workflow                                    # the workflows and their lanes
ramify new <slug> --workflow feature [--lane light] [--tag auth]
```

Pick the lane with the user. Take the lighter lane only when all of these are true:

- The spec states the bar in full.
- The repo has a precedent for the change.
- The work touches no invariant and no external contract.

A tag in the workflow's `min_lane` (auth, payments, permissions, personal data) forces the
heavier lane.

## Each step

1. `ramify step` shows the work order. The arrow (→) is the next step, with the skill to invoke.
2. Invoke that skill. It reads the work order and the outputs of earlier steps.
3. Record the step as its closing action: `ramify step done <step>`. A step that does not apply
   is `ramify step na <step> --note "<why>"`. Steps are recorded in lane order, so the record is
   the checklist: an entry means the step is done. The one exception is `finale`: it removes the
   worktree, so it is not recorded.

## Human gates

A step with `gate: human` (spec, qa-plan, review, diagnose, scope, frame, decide) ends with the
user's explicit OK. Show the result, ask, and wait. Record it only after the OK:
`ramify step done <step> --approved`. Silence, "looks fine so far" or your own judgement is not
an OK. Never pass `--approved` on your own.

## The lane goes up, never down

The work can turn out heavier than its lane: it touches auth, a public contract, a schema, or
data that cannot be rebuilt. Then stop and raise the lane: `ramify lane standard`. Say why in
the work order. Nothing lowers a lane.

## The PR waits on the record

`gh pr create` and `gh pr ready` are refused while a step before the PR step is missing. The
refusal names the next step. Do it. Do not route around the gate. A draft
(`gh pr create --draft`) is not gated, and is the way to share work in progress.

## Prose goes through ocre-jelly

Every piece of prose a step writes goes through the `ocre-jelly` skill before it is saved,
committed or sent. This covers the work order body, the spec, the QA plan and results, review
findings, code comments, commit messages and the PR body.

- Write the draft, then run `ocre-jelly` on it (rewrite mode). Use its `commit` mode for a
  commit message.
- Keep identifiers, paths, numbers and quoted UI text exactly as they are.
- Use the glossary of the repo you work in. ramify has none of its own. `ocre-jelly` finds the
  repo's `docs/ubiquitous-language.md` or `UBIQUITOUS-LANGUAGE.md` and holds the prose to its
  terms. A step that adds a domain term adds it to that glossary first.
- If the `ocre-jelly` skill is not installed, follow its rules by hand and say so once: short
  sentences, active voice, one idea per sentence, no filler, no recap, no "not X but Y".

## After a resume or a compaction

Run `ramify step` and `git status --short --branch` from the worktree before you edit. The
work order on disk is the state. Your memory of the session is not.
