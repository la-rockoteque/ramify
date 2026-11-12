---
workflow: feature
lane: standard
branch: feat/ram-code-edit-tool
tags: []
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12'}
- {step: qa, date: '2026-11-12'}
- {step: review, date: '2026-11-12'}
- {step: pr, date: '2026-11-12', note: '#63'}
---

# Ram Code edit tool

Ram Code gains `edit_file`, which replaces an exact string in a file. The model then changes a file without rewriting it whole. See [spec.md](spec.md).
