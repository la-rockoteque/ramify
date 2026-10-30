---
name: diagnose
description: Workflow step `diagnose` — reproduce the bug, find its root cause, write the failing tests that show it, and plan the fix in diagnose.md, then get the user's OK. Use when `ramify step` names diagnose as the next step.
---

# diagnose — the root cause before the fix

Follow the [workflow contract](../workflow/SKILL.md). This step is a **human gate**. Change no
product code in this step: a fix before the cause is a guess.

## Read first

- The work order (`ramify step` prints its path) and the bug report: the ticket, the logs, what
  the user saw.
- The code on the failing path. Trace it from the entry point to the wrong result.

## Do

1. Reproduce the bug. Write down the exact steps, input and output.
2. Find the root cause: the line or the rule that makes the wrong result. A symptom is not a
   cause. Ask "why" until the answer is in the code.
3. Write the tests that fail because of the bug, in the repo's test suite. Run them and see them
   fail for the right reason. Commit them.
4. Run the suite on main too. A test that fails there before your change is a known failure.
   Write it down; it is not this bug.

## Write `diagnose.md` next to the work order

- **Symptom**: what goes wrong, for whom, and how to reproduce it.
- **Risk**: what the bug can break or lose while it stays open.
- **Root cause**: the file, the function and the rule that cause it.
- **Failing tests**: the exact test names, and the file or suite section that holds them.
- **Fix**: the smallest change that makes them pass, in a few bullets.
- **Known failures, not in scope**: the failures main has too, with the evidence.

Keep the work order body to the summary: what is broken and why, with a link to `diagnose.md`.
Raise the lane (`ramify lane standard`) when the cause touches auth, a public contract, a
schema or data that cannot be rebuilt.

## Prose

Run `ocre-jelly` on `diagnose.md` and on the work order body before you show them.

## Gate

Put the diagnosis up for the user, then wait for the decision. The
[workflow contract](../workflow/SKILL.md#human-gates) has the loop:

```bash
ramify step ask diagnose
ramify step wait diagnose    # use the workflow contract’s runtime wait tools
```

On a rejection, apply the note and ask again. An explicit OK in the chat counts too: then
`ramify step done diagnose --approved`.
