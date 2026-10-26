---
workflow: feature
lane: standard
branch: feat/repo-workflows
tags: []
created: 2026-10-26
steps:
- {step: spec, date: 2026-10-26}
- {step: qa-plan, date: 2026-10-26}
- {step: build, date: 2026-10-26, note: '15 repository workflow tests, 6 Codex tests, and 21 Jira tests pass; smoke 187 passes and 2 baseline colour failures'}
- {step: audit, date: 2026-10-26, note: Every acceptance criterion and audit case has named evidence in qa-results.md}
- {step: qa, date: 2026-10-26, note: Real CLI/worktree/session fixtures and read-only Moship setup draft pass}
---

# repo-workflows

Detect a repository's existing development workflow during setup and use it as the source of truth.
Keep its documents, skills, and work records instead of creating a competing Ramify workflow.

See [spec.md](spec.md) and [qa-plan.md](qa-plan.md).
