---
name: ask-for-qa
description: Bring up the local stack for the current worktree and hand it to a human to QA, with the checklist of what changed. Use after a push, when a change is ready to look at, or in kickoff mode right after a worktree is cut. Containers are shared across sessions; the services a branch changes run per worktree.
argument-hint: "[kickoff|down]"
---

# Ask for QA

Put the branch in front of a human and get out of the way.

The tests do not know whether the panel feels right, whether the wording reads, or whether the
thing the ticket complained about is gone. This skill starts the stack for **this worktree**,
proves it answers, tells the reviewer exactly what to look at, and stops.

`ramify` must be on `PATH` (see the plugin README). If the repo has no `.ramify.conf` or
`.claude/ramify.conf`, run the `setup` skill first.

## Run it

```bash
RAMIFY_ROOT=<worktree> ramify up      # start, wait for ready, print the card
RAMIFY_ROOT=<worktree> ramify card    # this worktree's URLs and health
RAMIFY_ROOT=<worktree> ramify down    # stop this worktree's own services, release its slot
```

Always pass `RAMIFY_ROOT` with the absolute worktree path. An agent shell resets its directory
between commands, and a `down` aimed at the wrong tree kills nothing and reports success.

`up` is idempotent: re-run it after a rebuild rather than inventing a restart. It ends by
printing the card, so the URLs are the last thing on screen. **Paste the card into the reply in
a fenced block** — tool output reaches you, not reliably the human.

## What is shared and what is not

- **Containers** (`RAMIFY_CONTAINERS`) run once, under a fixed compose project. Never
  `docker compose down`: another session is likely mid-QA against that database.
- **A service with `<svc>_shared_port`** runs once from the primary checkout while the branch
  changes nothing outside `<svc>_shared_when`. Its `<svc>_prepare` (migrations, say) runs only
  when the branch gets a private instance, because migrating the shared database changes what
  every other reviewer sees. `ramify shared-down` is the deliberate way to stop it.
- **Everything else is per worktree**, on a slot from a machine-wide registry: slot N is
  `<svc>_port + N`. `ramify status` prints the table.

## Kickoff mode

Right after a worktree is cut, the hooks start its stack in the background. Once `card`
answers, hand over the URLs and the checklist so the reviewer can watch the work land, then
**keep implementing** — kickoff does not wait. The hand-over after the push does.

## Then ask

Once the URLs answer, tell the reviewer **what to look at**, not that the stack runs.

1. Read the QA plan when the repo has one: the `qa-plan.md` next to the branch's work order
   (`ramify step` prints its path), else `ramify config get RAMIFY_QA_PLAN`, a glob with
   `<slug>` standing for the worktree name. Its cases are the checklist.
2. Otherwise derive the list from the diff: which screens and behaviours changed, and what a
   test run could not show.
3. Name the **route** for each item, not only the component.
4. Call out anything visible but not obviously in scope, especially shared components.
5. Say what you could **not** verify yourself and why.
6. Add `RAMIFY_QA_NOTES` when the config has it (seed data, test accounts).
7. Run the hand-over text through the `ocre-jelly` skill before you send it. Keep the URLs,
   routes and quoted UI text as they are.

Then stop and wait. Do not start the next task, poll logs, or summarise the work again.

## When they come back

- Something is wrong → fix it, re-run `up`, and say what changed.
- Everything passes → record it where `RAMIFY_QA_RESULTS` points, if set, then `ramify down`.
- Leave the containers running either way.

## When it does not come up

- `NOT READY` → read the log path it prints. Readiness is an answered `<svc>_health` request,
  not a started process.
- `smoke … <-- check the wiring` → the services started but do not reach each other. A later
  service reads an earlier one's port as `$<SERVICE>_PORT`; check the config's command uses it.
- `all N slots taken` → `ramify dash`, then `ramify cleanup` for what nothing runs behind.
