---
workflow: feature
lane: standard
branch: feat/jira-lifecycle
tags: []
created: 2026-10-26
steps:
- {step: spec, date: 2026-10-26}
- {step: qa-plan, date: 2026-10-26}
- {step: build, date: 2026-10-26, note: 21 lifecycle tests and 6 Codex tests pass; smoke passes 187 cases with 2 documented baseline colour failures}
- {step: audit, date: 2026-10-26, note: Every acceptance criterion and QA audit case has named evidence in qa-results.md}
- {step: qa, date: 2026-10-26, note: 'Live local fixture verifies automatic assignment, progress, review, and done; legacy Jira checks pass'}
- {step: review, date: 2026-10-26}
---

# jira-lifecycle

Assign a Jira story when work starts and synchronize its status with GitHub, GitLab, or Bitbucket Cloud.
Move the story into review when its request opens and into done when the host confirms a merge.

See [spec.md](spec.md) for behaviour and [qa-plan.md](qa-plan.md) for checks.
