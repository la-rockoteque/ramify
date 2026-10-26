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

## CLI and runtime tools

These skills work in Claude Code and Codex. Use the CLI bundled with the installed plugin.
Resolve its absolute path from this skill's location: `../../bin/ramify` relative to its directory.
Use that path wherever a command below says `ramify`, even when another CLI is on `PATH`.
A CLI on `PATH` can belong to an older installation for the other agent.
Do not assume a hook's environment variables exist in a skill's shell.

For a wait command, use the runtime's background process tools:

- Claude Code: use Bash with `run_in_background` and read the completion output.
- Codex: start `exec_command` with a short `yield_time_ms`. Keep its returned `session_id` and collect completion with `write_stdin`.
- In Codex code mode, await the tool call inside `functions.exec`. If that cell yields, resume it with `functions.wait`.
- If the runtime cannot keep a process, use `--timeout 45` and report that the decision remains pending after a timeout.

Keep one wait process per request. Do not start repeated wait commands or query the approval state in a loop.
A timeout is not approval. Resume from the work order when the human answers.

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
user's explicit OK. Put the gate up in the approvals window, then wait for the decision:

1. Write the step's file next to the work order (`spec.md`, `qa-plan.md`, `review.md`, or
   `<step>.md`). Run `ocre-jelly` on it first.
2. `ramify step ask <step>`. The approvals window opens on the user's screen with the file and
   the work order. Add `--file <path>` for each other file the user must read.
3. Tell the user in one line that the step waits on them, and name the file.
4. Run `ramify step wait <step>` with the runtime tools above. Read its completion output when the human decides.
5. Read what it printed:
   - `approved` (maybe with a note): the step is already recorded. Take the note into account,
     then go on with the next step.
   - `rejected: <note>`: apply the note, update the file, and go back to step 2.

The user can also answer in the chat. After an explicit OK there, record it yourself:
`ramify step done <step> --approved`. That also closes the window's request. Silence, "looks
fine so far" or your own judgement is not an OK. Never pass `--approved` on your own.

`ramify approve` and `ramify reject` belong to the human. The plugin refuses them from the agent.

## Scripts that need a human

Some commands are not yours to run: a write to production, a data import, a change to a shared
account. When a permission rule or the user blocks such a command, do not route around it.

1. Write the commands as one script. Make it stop on the first error (`set -euo pipefail`).
2. `ramify run ask <script> --note "<what it does, in one line>"`. The run window opens on the
   user's screen with the script.
3. Tell the user in one line that the script waits on them.
4. Run `ramify run wait <id>` with the runtime tools above.
5. Read what it printed: `ran: exit <n>` with the output, or `blocked: <reason>`.

Never edit the frozen copy in the runs folder. To change the script, ask again with a new one.
`ramify run go` and `ramify run block` belong to the human. The plugin refuses them from the agent.

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

Use the [resume skill](../resume/SKILL.md) from the existing worktree before you edit.
The work order and files on disk hold the shared state across agents and sessions.
