---
workflow: feature
lane: standard
branch: feat/allow-ramify-bash
tags: [permissions]
created: 2026-10-28
steps:
- {step: spec, date: '2026-10-28'}
- {step: qa-plan, date: '2026-10-28'}
- {step: build, date: '2026-10-28'}
---

# allow-ramify-bash

A PreToolUse hook allows plain `ramify` commands without a permission prompt. Commands that
belong to the human, remove work or write outside the worktree still prompt. See
[spec.md](spec.md).
