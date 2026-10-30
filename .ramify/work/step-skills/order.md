---
workflow: feature
lane: light
branch: feat/step-skills
tags: []
created: 2026-10-30
steps:
- {step: spec, date: '2026-10-30'}
- {step: qa-plan, date: '2026-10-30'}
- {step: build, date: '2026-10-30'}
---

# step-skills

`workflows.yml` names eight step skills that `skills/` does not have: `ramify:diagnose`,
`ramify:fix`, `ramify:verify`, `ramify:scope`, `ramify:apply`, `ramify:frame`,
`ramify:prototype` and `ramify:decide`. The bug, tweak and spike workflows point the agent at
skills that do not exist. This branch writes them, lists them in the README, and adds a test
that every `skill:` in `workflows.yml` has its `SKILL.md`. See [spec.md](spec.md).
