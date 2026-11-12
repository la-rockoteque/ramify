---
workflow: feature
lane: standard
branch: feat/ram-hooks
tags: []
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12', note: AC16 interface path is Q22 (live)}
---

# ram-hooks

Ram Code runs no hooks, so a gate, guard or context note that works in Claude Code does nothing
there. This branch builds Claude Code's hook mechanics into Ram Code. It reads hooks from
`~/.ram`, `.ram`, Claude Code's settings and enabled plugins, and ramify's `hooks/ram.json`.
Matchers use Ram Code's tool names. Stacked on `feat/zsh-prompt`. See [spec.md](spec.md).
