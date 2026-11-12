---
workflow: feature
lane: standard
branch: feat/zsh-prompt
tags: []
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
---

# zsh-prompt

A zsh prompt segment that shows the ramify state of the current directory and the live Claude
Code and Codex sessions in its worktree. It replaces barbarbar, a Rust oh-my-zsh segment whose
source is lost. It ships as an oh-my-zsh plugin and a powerlevel10k segment. See [spec.md](spec.md).
