---
name: prototype
description: Workflow step `prototype` — build the least code that answers the spike's question inside its time box, and write the answer and its evidence in finding.md. Use when `ramify step` names prototype as the next step.
---

# prototype — the least code that answers the question

Follow the [workflow contract](../workflow/SKILL.md).

## Read first

`frame.md`: the question, the time box and what answers it.

## Do

1. Build only what the experiment in `frame.md` needs. Hard-code, stub and copy freely: this
   code does not merge.
2. Run the experiment. Keep the commands, the numbers and the output.
3. Stop at the end of the time box, answered or not.
4. Commit as you go, so the finding can point at the code.

Run nothing that writes to production or a shared account. When the experiment needs it, use
the [run skill](../run/SKILL.md) and let the user run it.

## Write `finding.md` next to the work order

- **Answer**: yes, no, a number, or "not known". One line.
- **Evidence**: what you ran and what it gave: commands, numbers, commits.
- **Cost**: what the real build would take, and the risks the prototype found.
- **Not known**: what the time box left open.

## Prose

Run `ocre-jelly` on `finding.md`.

## Record

When `finding.md` holds the answer, or says why there is none:

```bash
ramify step done prototype
```
