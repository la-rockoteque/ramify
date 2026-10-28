# Session names QA plan

The automated cases are `check` lines in `test/smoke.sh`. Each one runs the `session-start` or
`session-end` hook on the throwaway repo, with `RAMIFY_STATE_DIR` in the test folder. The check
name is the exact string below. No case touches `/tmp/ramify` on the machine.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `session-start writes a name from trees` | build |
| Q2 | AC1 | medium | `the name file holds one line` | build |
| Q3 | AC2 | high | `two sessions get two names` | build |
| Q4 | AC3 | high | `a full theme adds -2` | build |
| Q5 | AC4 | medium | `RAMIFY_SESSION_NAMES picks a built-in theme` | build |
| Q6 | AC5 | medium | `RAMIFY_SESSION_NAMES takes a word list` | build |
| Q7 | AC6 | high | `RAMIFY_SESSION_NAMES=off writes no name` | build |
| Q8 | AC6 | high | the existing title checks pass unchanged with `RAMIFY_SESSION_NAMES=off` | build |
| Q9 | AC7 | high | `session-start titles a worktree slug · name` | build |
| Q10 | AC8 | high | `session-start titles the primary with the name` | build |
| Q11 | AC9 | high | `a custom title keeps the name file` | build |
| Q12 | AC10 | high | `session-end releases the name` | build |
| Q13 | AC10 | medium | `prompt_input_exit keeps the name` | build |
| Q14 | AC11 | high | `resume gives the old name back` | build |
| Q15 | AC11 | medium | `resume takes a new name when the old one is held` | build |
| Q16 | AC12 | high | `no config, no name` | build |
| Q17 | AC13 | medium | `statusline --json has the name` and `statusline --json name is null without one` | build |
| Q18 | AC13 | low | `the plain status line starts with the name` | build |
| Q19 | — | medium | `an unwritable state dir names nothing and exits 0` | build |
| Q20 | — | medium | `every built-in theme has one word per line, no duplicates` | build |
| Q21 | AC14 | low | Read the README against the hook: variable, themes, file path, `ended/`, title format | audit |
| Q22 | AC1–AC13 | medium | Read the diff: the hook never fails a turn, quotes every path, and parses under `/bin/bash` 3.2 | audit |
| Q23 | AC7, AC8 | high | Start a real Claude Code session in a worktree of this repo and one in the primary checkout. The session list shows `session-names · <name>` and `<name>`, and two names differ | qa |
| Q24 | AC10, AC11 | medium | End the worktree session, resume it with `claude --resume`. The name file is back in `sessions/` with the same name | qa |

## Write set

```text
hooks/ramify-hook
bin/ramify
libexec/statusline.sh
assets/names/*
test/smoke.sh
README.md
skills/setup/*
.ramify/work/session-names/*
!app/*
!libexec/workflow.py
!.ramify/work/codex-handoff/*
!.ramify/work/jira-lifecycle/*
```

`bin/ramify` is in the set for the `ramify config` warning about an unknown theme word.
`skills/setup/*` is in the set so the setup skill can mention the setting.
