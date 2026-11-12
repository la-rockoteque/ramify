---
workflow: feature
lane: standard
branch: feat/ram-code-search-tools
tags: []
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12'}
- {step: qa, date: '2026-11-12'}
- {step: review, date: '2026-11-12'}
---

# Ram Code search tools

Ram Code gains three read-only tools: `grep`, `glob` and a tree-sitter `code_map`. The model then finds code without reading file after file or using the shell. See [spec.md](spec.md).
