---
workflow: feature
lane: standard
branch: feat/zsh-prompt
tags: []
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12', note: AC16 real p10k is Q23; amendment built in ba5301c}
- {step: qa, date: '2026-11-12', note: Q33 waived by the user}
- {step: review, date: '2026-11-12'}
- {step: pr, date: '2026-11-12', note: '#52'}
---

# zsh-prompt

A zsh prompt segment that shows the ramify state of the current directory and the live Claude
Code and Codex sessions in its worktree. It replaces barbarbar, a Rust oh-my-zsh segment whose
source is lost. It ships as an oh-my-zsh plugin and a powerlevel10k segment. See [spec.md](spec.md).
