---
name: finale
description: Workflow step `finale` — after a human merges the PR, prove the branch landed, then close its ticket and remove the worktree and branch. Use when `ramify step` names finale as the next step, or when told a work order's PR is merged.
---

# finale — tear down after the merge

Follow the [workflow contract](../workflow/SKILL.md). Run it only after a human has merged the
PR. If it is not merged, stop: nothing here is safe before the merge.

## Prove it landed

```bash
gh pr view <number> --json state,mergedAt,mergeCommit
git fetch origin
git diff --stat HEAD origin/main -- $(git diff --name-only "$(git merge-base HEAD origin/main)" HEAD)
```

A squash merge leaves no shared commit, so check by content, not ancestry. The last command
compares the files the branch changed with main. An empty result means they landed. A
difference must come from a later commit on main (`git log HEAD..origin/main -- <file>`). If
it does not, stop and say what is missing.

## Tear down

Do not record `finale`. The teardown removes the worktree and its work order, and `complete`
refuses a tree with uncommitted or unpushed changes. The work order on main ends at `pr`, and
the removed branch is the record of the finale. From the primary checkout:

```bash
ramify complete <worktree>      # closes the ticket, stops the stack, removes tree and branch
```

`complete` refuses uncommitted or unpushed work. Read why before you reach for `ramify delete`.

## Prose

Run `ocre-jelly` on any message you post to the PR or the ticket.
