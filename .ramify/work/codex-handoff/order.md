---
workflow: feature
lane: standard
branch: feat/codex-handoff
tags: []
created: 2026-10-25
steps:
- {step: spec, date: 2026-10-25}
- {step: qa-plan, date: 2026-10-25}
- {step: build, date: 2026-10-26, note: 6 Codex tests pass; 187 smoke cases pass; 2 colour failures reproduced on unchanged main}
- {step: audit, date: 2026-10-26, note: Acceptance criteria and audit cases have named evidence in qa-results.md}
- {step: qa, date: 2026-10-26, note: Fresh Codex discovery and read-only recovery pass; 6 regressions pass; 2 unchanged baseline colour failures documented}
- {step: review, date: 2026-10-26}
- {step: pr, date: 2026-10-26, note: '#20'}
---

# codex-handoff

Let Codex continue work that Claude Code started in the same worktree.
Use the existing work order, unfinished edits, approvals, and QA state.

See [spec.md](spec.md) for behaviour and acceptance criteria.
