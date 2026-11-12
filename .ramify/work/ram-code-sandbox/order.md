---
workflow: feature
lane: standard
branch: feat/ram-code-sandbox
tags: [permissions]
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12'}
- {step: qa, date: '2026-11-12'}
---

# Ram Code sandbox

`run_command` runs in a sandbox with no network and writes only in the worktree, with timeouts and provider keys removed. A command that must leave the sandbox goes to the human through `ramify run`, highlighted in the TUI's Run pane. See [spec.md](spec.md).
