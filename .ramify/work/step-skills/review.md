# Review: the bug, tweak and spike step skills

A fresh subagent reviewed the diff against `spec.md` and `qa-plan.md`. It found no gap against
AC1 to AC6. It ran four mutations on `test/skills.py`, and each one made a test fail. These are
its findings and what this branch did with each.

| # | Severity | Where | Finding | Fix |
|---|---|---|---|---|
| R1 | high | `skills/diagnose/SKILL.md` | With `auto_ask: true`, the guard holds edits outside the planning paths while `diagnose` is next. The agent cannot write the failing tests in that step. The request also opens as soon as `diagnose.md` exists. | `diagnose` now says: under `auto_ask`, name the tests in `diagnose.md`, and write `diagnose.md` last. `fix` step 1 writes tests that `diagnose` only named. |
| R2 | medium | `skills/decide/SKILL.md` | `ramify delete` refuses from inside the worktree. The skill did not say where the user runs it. | It now says: from the primary checkout, in a terminal. |
| R3 | medium | `skills/decide/SKILL.md` | A spike is never pushed, so the delete loses `finding.md`. | Keep the finding first: in the new work order for a seed, on the ticket or through the scratch skill for a discard. |
| R4 | low | `skills/scope/SKILL.md`, `skills/apply/SKILL.md` | A tweak that turns into a feature or a bug left its worktree in place, and `ramify new` did not say where to run. | Both say "from the primary checkout", and that the user removes the tweak worktree with `ramify delete`. |
| R5 | low | `skills/decide/SKILL.md` | "the decision … the decision". | "Put `decide.md` up for the user, then wait for the answer." |
| R6 | medium | `skills/audit/SKILL.md` | The bug paragraph said there is no QA plan, but the rest still writes `qa-results.md` and checks the write set. | On a bug, the verdicts go to `audit.md`, with no plan cases and no write set. |
| R7 | medium | `skills/review/SKILL.md` | It gave the reviewer `spec.md`, `qa-plan.md`, `qa-results.md`, which bug and tweak work orders do not have. It said only the light lane has no audit. | It gives `spec.md` (or `diagnose.md`, `scope.md`) and the QA record when there is one. It names each lane without audit. This file was not in the write set; the write set now lists it. |
| R8 | medium | `skills/pr/SKILL.md` | Acceptance and QA lines assumed a spec and `qa-results.md`. | Acceptance names the failing tests for a bug and the `scope.md` list for a tweak. QA cites `verify.md` for a bug. |
| R9 | low | `test/skills.py` | A step with no `skill:` raised `KeyError`. A non-`ramify:` skill passed. | Both are now reported as problems. |
| R10 | low | `test/skills.py` | Only a missing file had a negative test. | `test_wrong_name_and_description_fail` covers a wrong `name`, a bad description and a step with no skill. |
| R11 | low | `test/skills.py` | A BOM or CRLF file read as a name mismatch. | Frontmatter parsing strips both. |
| R12 | low | `test/skills.py` | Nothing checked that `finale` does not record itself. | `test_auto_skills_record_done` now checks it. |
| R13 | low | `test/smoke.sh` | Nothing ran `test/skills.py`. | Smoke runs it near the top. The write set now lists `test/smoke.sh`. |

## Not fixed, out of scope

- `workflows.yml` `audit` says "the spec or the report". The write set keeps `workflows.yml`
  as it is.
- `skills/workflow/SKILL.md` lists only the feature step files. It is not in the write set.

## Checks after the fixes

- `python3 test/skills.py`: 7 tests, OK.
- Each `test/*.py` and `test/*.js`: pass, except `test/codex.py`
  `test_resume_preserves_unfinished_build`, which fails on main too.
- `test/smoke.sh`: see below.
- `ramify write-set .ramify/work/step-skills/qa-plan.md`: clean.
- ocre-jelly `scan.py`: 0 candidates on each new `SKILL.md` and on this file.
