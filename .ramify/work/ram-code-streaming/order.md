---
workflow: feature
lane: standard
branch: feat/ram-code-streaming
tags: []
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12'}
---

# Ram Code streaming and interrupt

Ram Code streams model replies for both API formats, so text shows as it arrives and long replies need no request timeout. Esc stops a running task and keeps what finished, with a history that the next request accepts. See [spec.md](spec.md).
