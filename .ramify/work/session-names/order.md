---
workflow: feature
lane: standard
branch: feat/session-names
tags: []
created: 2026-10-28
steps:
- {step: spec, date: 2026-10-28}
- {step: qa-plan, date: '2026-10-28'}
- {step: build, date: '2026-10-28'}
- {step: audit, date: '2026-10-28'}
- {step: qa, date: '2026-10-28'}
- {step: review, date: '2026-10-28'}
- {step: pr, date: '2026-10-28', note: '#25'}
---

# session-names

Give each agent session a short name from a theme, such as `birch`, so the user can tell
sessions apart. The name goes into the session title and into a file that status lines, such as
barbarbar, read. The default theme is `trees`. See [spec.md](spec.md).
