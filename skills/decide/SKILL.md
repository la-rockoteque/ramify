---
name: decide
description: Workflow step `decide` — with the user, discard the spike or seed a feature work order from its finding, then remove the spike's worktree. A spike opens no PR. Use when `ramify step` names decide as the next step.
---

# decide — discard the spike, or seed a feature from it

Follow the [workflow contract](../workflow/SKILL.md). This step is a **human gate**, and the last
step of a spike. A spike opens no PR and has no `finale`: this step removes its worktree.

## Read first

`frame.md` and `finding.md`.

## Write `decide.md` next to the work order

- **Decision**: discard, or seed a feature.
- **Why**: one or two lines from `finding.md`.
- **Seed**: for a feature, its slug and its first lines of what and why.

## Prose

Run `ocre-jelly` on `decide.md` before you show it.

## Gate

Put `decide.md` up for the user, then wait for the answer. The
[workflow contract](../workflow/SKILL.md#human-gates) has the loop:

```bash
ramify step ask decide
ramify step wait decide      # use the workflow contract’s runtime wait tools
```

On a rejection, apply the note and ask again. An explicit OK in the chat counts too: then
`ramify step done decide --approved`.

## After the OK

Nothing in the plugin stops a PR from a spike. Do not open one.

To seed a feature, from the primary checkout:

```bash
ramify new <slug> --workflow feature
```

The new branch starts from main, without the spike's code. Copy `frame.md` and `finding.md`
into its work order folder, and link them from its body. Its `spec` step starts from them.

Nothing of the spike is pushed, so the delete loses `frame.md`, `finding.md` and `decide.md`.
Keep the finding first. For a seed, it is in the new work order. For a discard, post it on the
ticket, or hand it to the user with the [scratch skill](../scratch/SKILL.md).

Then the user removes the spike, from the primary checkout, in a terminal. `ramify delete`
refuses from inside the worktree, and the branch is not merged:

```bash
ramify delete <spike-worktree>
```
