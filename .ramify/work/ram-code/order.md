---
workflow: feature
lane: standard
branch: feat/ram-code
tags: []
created: 2026-11-01
steps:
- {step: spec, date: '2026-11-01'}
- {step: qa-plan, date: '2026-11-01'}
- {step: build, date: '2026-11-01'}
- {step: audit, date: '2026-11-01'}
- {step: qa, date: '2026-11-01', na: true, note: this repo declares no services; every QA case is settled by build}
---

# Ram Code

Build a terminal coding harness in the Ramify repository. It uses Ramify directly for worktree and workflow operations, starts with Claude Opus as coordinator, and routes delegated work to DeepSeek through the Nexapp API. Codex remains an optional provider. See [spec.md](spec.md).
