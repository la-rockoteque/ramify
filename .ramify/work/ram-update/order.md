---
workflow: feature
lane: standard
branch: feat/ram-update
tags: []
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12'}
- {step: qa, date: '2026-11-12', note: Q17 waived by the user}
---

# ram-update

Ram Code checks for a newer release, or for new commits on `main` in a checkout. It shows the
update in its header, and `ram update` installs it. Auto-update at quit is an opt-in setting.
See [spec.md](spec.md).
