# QA plan: the bug, tweak and spike step skills

The light lane has no audit step. Each case is settled by `build` or by `qa`. Here `qa` means a
read of the files and a run of the suite: the change adds no service to a QA stack.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1, AC5 | high | `test/skills.py` `test_every_workflow_skill_has_its_skill_file` | build |
| Q2 | AC1, AC5 | high | `test/skills.py` `test_skill_name_matches_its_folder` | build |
| Q3 | AC1, AC5 | medium | `test/skills.py` `test_step_skill_description_names_its_step` | build |
| Q4 | AC5 | high | `test/skills.py` `test_missing_skill_fails`: a temp `workflows.yml` that names `ramify:nothing` makes the check report it | build |
| Q5 | AC2 | high | `test/skills.py` `test_gated_skills_ask_and_wait`: each `gate: human` step's skill names `<step>.md`, `ramify step ask <step>` and `ramify step wait <step>` | build |
| Q6 | AC3 | medium | `test/skills.py` `test_auto_skills_record_done`: each auto step's skill names `ramify step done <step>` | build |
| Q7 | AC4 | medium | Read `skills/decide/SKILL.md`: it says a spike opens no PR and has no `finale`, and gives the discard and the seed teardown | qa |
| Q8 | AC6 | low | Read `README.md`: it lists the eight new skills next to the feature ones | qa |
| Q9 | AC7 | medium | `scan.py` from ocre-jelly reports 0 hard hits on each new `SKILL.md` | qa |
| Q10 | AC7 | high | `bash test/smoke.sh`, each `test/*.py` with `python3`, each `test/*.js` with `node`. Only the failures known on main may fail | qa |
| Q11 | spec point 3 | low | `grep -rn report.md skills/` finds nothing | qa |

## Write set

`ramify write-set` reads each bare line as a glob. A `!` line is a path the branch must not
change.

```
skills/diagnose/SKILL.md
skills/fix/SKILL.md
skills/verify/SKILL.md
skills/scope/SKILL.md
skills/apply/SKILL.md
skills/frame/SKILL.md
skills/prototype/SKILL.md
skills/decide/SKILL.md
skills/audit/SKILL.md
skills/pr/SKILL.md
test/skills.py
README.md
.ramify/work/step-skills/*
!libexec/*
!bin/*
!hooks/*
!workflows.yml
!.claude-plugin/*
!.codex-plugin/*
```
