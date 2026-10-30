# Spec: the bug, tweak and spike step skills

## Problem

`workflows.yml` names a skill for each step. Eight of them do not exist: `ramify:diagnose`,
`ramify:fix`, `ramify:verify`, `ramify:scope`, `ramify:apply`, `ramify:frame`,
`ramify:prototype` and `ramify:decide`. `ramify step` tells the agent to invoke them on a bug,
tweak or spike branch. The agent then has no contract and guesses the files and the gates.

## Behaviour

Each skill is `skills/<step>/SKILL.md`. It has the same shape, tone and length as the feature
step skills (`spec`, `qa-plan`, `build`, `audit`, `review`, `pr`, `finale`):

- Frontmatter: `name: <step>` and a `description` that starts "Workflow step `<step>` —" and
  ends "Use when `ramify step` names <step> as the next step."
- A link to the workflow contract, and whether the step is a human gate.
- What to read first, what to do, the file it writes next to the work order, and how to record
  the step.

| Skill | Workflow and lanes | Gate | File it writes |
|---|---|---|---|
| `diagnose` | bug: hotfix, standard | human | `diagnose.md`: symptom, risk, root cause, failing tests, fix plan |
| `fix` | bug: hotfix, standard | auto | none; the commits |
| `verify` | bug: standard | auto | `verify.md`: each check, result, evidence; known failures with main's result |
| `scope` | tweak | human | `scope.md`: what changes, why, what is not in it, why it is a tweak |
| `apply` | tweak | auto | none; the commits |
| `frame` | spike | human | `frame.md`: the question, the time box, what answers it |
| `prototype` | spike | auto | `finding.md`: the answer, the evidence, what it costs |
| `decide` | spike | human | `decide.md`: discard, or the feature to seed |

Step rules:

- `fix` makes the smallest change that makes the failing tests from `diagnose.md` pass. No
  refactor, no cleanup. On the hotfix lane it also runs the checks near the change, because
  there is no `verify`.
- `verify` runs the repo's whole suite, build, type-check and lint, as the repo defines them. A
  failure that also happens on main is recorded as known, with evidence from a run on a clean
  checkout of main.
- `apply` makes the change `scope.md` describes and commits as it goes. It raises nothing: a
  tweak that grows into a feature or a bug starts a new work order.
- `decide` asks the user. Discard: the user deletes the spike worktree with `ramify delete`.
  Seed: `ramify new <slug> --workflow feature`, with `finding.md` copied into the new work order,
  then the spike is deleted. A spike opens no PR.

## Registration

- The plugin manifests find skills by folder (`.codex-plugin/plugin.json` has
  `"skills": "./skills/"`; Claude Code reads `skills/` by default). They need no change.
- The README lists the workflow skills. It gets the eight new ones, grouped by workflow.
- `test/skills.py` checks that each `skill:` in `workflows.yml` has `skills/<name>/SKILL.md`
  whose frontmatter `name` is `<name>` and whose `description` starts "Workflow step `<step>`".

## Acceptance criteria

- AC1: `skills/<step>/SKILL.md` exists for each of the eight steps, with `name: <step>` and a
  description in the shape above.
- AC2: Each gated skill (`diagnose`, `scope`, `frame`, `decide`) writes `<step>.md`, raises the
  gate with `ramify step ask <step>`, and waits with `ramify step wait <step>`. This is the file
  `workflow.py` expects by default (`files: [<step>.md]`), and `ask` fails without it.
- AC3: Each auto skill (`fix`, `verify`, `apply`, `prototype`) records with
  `ramify step done <step>`.
- AC4: `decide` says the spike opens no PR and has no `finale`, and gives the teardown for
  discard and for seed.
- AC5: `test/skills.py` passes, and fails when a `skill:` in `workflows.yml` has no
  `SKILL.md` or a different `name`.
- AC6: The README lists every workflow step skill.
- AC7: All new prose follows ocre-jelly. The whole suite passes, except the failures known on
  main.

## Where `workflows.yml` and the code disagree

The code wins. This branch changes no code. It notes these:

1. `workflows.yml` says the spike opens "no PR". `workflow.py` `cmd_gate` lets a lane without an
   `opens_pr` step open PRs freely. Nothing stops a PR from a spike. The `decide` skill states
   the rule, and nothing enforces it.
2. The spike lane has no `finale`. Nothing removes a spike's worktree. `decide` carries the
   teardown.
3. The `audit` and `pr` skills read `report.md` for a bug. No step writes `report.md`: the bug
   lanes start with `diagnose`, and its gate file is `diagnose.md`. This branch changes those
   two references to `diagnose.md`.

## Out of scope

- Changes to `workflow.py`, the hooks or `workflows.yml`.
- A check in code that a spike opens no PR.
- A plugin version bump.

## Open questions

- Point 3 above: change `report.md` to `diagnose.md` in `audit` and `pr`? This spec says yes.

## Lane

Light. The change is prose and one test. The repo has seven step skills as precedent. It
touches no invariant and no external contract.
