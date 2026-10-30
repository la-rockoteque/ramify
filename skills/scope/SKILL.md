---
name: scope
description: Workflow step `scope` — write what a tweak changes and why into scope.md, check that it is neither a feature nor a bug, then get the user's OK. Use when `ramify step` names scope as the next step.
---

# scope — what changes, and why it is only a tweak

Follow the [workflow contract](../workflow/SKILL.md). This step is a **human gate**.

## Read first

- The work order (`ramify step` prints its path) and what the user asked for.
- The code or the config the tweak touches.

## Check it is a tweak

A tweak changes no behaviour a user relies on and fixes no broken behaviour. It touches no
schema, no public contract, no auth and no data that cannot be rebuilt. Examples: a CI setting,
a dependency pin, a log line, a rename inside one module, a doc fix.

- New behaviour is a feature: `ramify new <slug> --workflow feature`.
- Broken behaviour is a bug: `ramify new <slug> --workflow bug`.

Then tell the user, and stop this work order.

## Write `scope.md` next to the work order

- **What changes**: numbered, one change per item, with the file each one touches.
- **Why**: the reason, with the evidence: the error, the number, the link.
- **Not in this tweak**: what is near but stays out, and why.
- **Why it is a tweak**: one or two lines against the check above.

Keep the work order body to the summary: what and why, with a link to `scope.md`.

## Prose

Run `ocre-jelly` on `scope.md` and on the work order body before you show them.

## Gate

Put the scope up for the user, then wait for the decision. The
[workflow contract](../workflow/SKILL.md#human-gates) has the loop:

```bash
ramify step ask scope
ramify step wait scope       # use the workflow contract’s runtime wait tools
```

On a rejection, apply the note and ask again. An explicit OK in the chat counts too: then
`ramify step done scope --approved`.
