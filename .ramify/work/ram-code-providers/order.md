---
workflow: feature
lane: standard
branch: feat/ram-code-providers
tags: [auth]
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
---

# Ram Code providers

Ram Code switches between named model providers from inside the app: Claude, OpenAI, DeepSeek, today's environment setup, and custom ones such as the team's LiteLLM. The user no longer edits the environment and restarts to change models. It builds on #53. See [spec.md](spec.md).
