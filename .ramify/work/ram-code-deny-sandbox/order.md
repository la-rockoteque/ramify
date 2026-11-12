---
workflow: feature
lane: standard
branch: feat/ram-code-deny-sandbox
tags: [permissions]
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12'}
- {step: qa, date: '2026-11-12'}
- {step: review, date: '2026-11-12'}
- {step: pr, date: '2026-11-12', note: '#66'}
---

# Ram Code deny-by-default sandbox

The macOS sandbox starts from `(deny default)` and allows only what builds, tests and Git need. Reads are limited to system, worktree and toolchain roots, and users extend them in `sandbox.json`. Every #62 guarantee stays. See [spec.md](spec.md).
