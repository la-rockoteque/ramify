---
name: frame
description: Workflow step `frame` — state the one question a spike answers, its time box and what would answer it in frame.md, then get the user's OK. Use when `ramify step` names frame as the next step.
---

# frame — one question, a time box, and what answers it

Follow the [workflow contract](../workflow/SKILL.md). This step is a **human gate**. A spike is
a throwaway prototype: it opens no PR, and its code does not merge.

## Read first

- The work order (`ramify step` prints its path) and what the user wants to know.
- The code, the library or the service the question is about.

## Write `frame.md` next to the work order

- **Question**: one question, with a yes, a no or a number as its answer. Split a wider
  question into several spikes.
- **Why it matters**: the decision that waits on the answer.
- **Time box**: the most time the prototype gets. At the end of it, the finding says what is
  known, even when the answer is not.
- **What answers it**: the experiment, and the result that means yes and the one that means no.
- **Out of scope**: what the prototype does not build, such as error paths, tests and polish.

Keep the work order body to the summary: the question and why, with a link to `frame.md`.

## Prose

Run `ocre-jelly` on `frame.md` and on the work order body before you show them.

## Gate

Put the frame up for the user, then wait for the decision. The
[workflow contract](../workflow/SKILL.md#human-gates) has the loop:

```bash
ramify step ask frame
ramify step wait frame       # use the workflow contract’s runtime wait tools
```

On a rejection, apply the note and ask again. An explicit OK in the chat counts too: then
`ramify step done frame --approved`.
