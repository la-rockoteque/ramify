---
workflow: feature
lane: light
branch: feat/setup-detection
tags: []
created: 2026-10-30
steps:
- {step: spec, date: '2026-10-30'}
- {step: qa-plan, date: '2026-10-30'}
- {step: build, date: '2026-10-30'}
- {step: review, date: '2026-10-30'}
- {step: pr, date: '2026-10-30', note: '#30'}
---

# setup-detection

`ramify setup --print` drafted configs for 19 real repos. Most drafts were wrong or empty, and each config needed a hand rewrite. This branch makes the draft detect the install, the dev servers, free ports, env files, compose files, the branch prefix and mobile apps.

The spec is in [spec.md](spec.md).
