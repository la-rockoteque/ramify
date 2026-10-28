# Allow ramify commands QA plan

The automated cases are `check` lines in `test/smoke.sh`. Each one pipes a PreToolUse JSON
input to `ramify-hook allow` and reads the decision. "allow" means stdout holds
`"permissionDecision": "allow"`. "none" means stdout is empty.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `allow lets ramify step through` | build |
| Q2 | AC2 | high | `allow takes cd and a RAMIFY_ prefix` | build |
| Q3 | AC3 | medium | `allow takes a pipe to head and 2>&1` | build |
| Q4 | AC4 | high | `allow leaves every prompted command alone`: one input per row of "Still prompted", including `step done review --approved` and `prune --apply` | build |
| Q5 | AC5 | high | `allow leaves a chain with another command alone`: `ramify step && rm -rf x`, `ramify step; curl x`, `ramify step \| sh` | build |
| Q6 | AC6 | high | `allow leaves another ramify binary alone`: `PATH=/tmp ramify step`, `./bin/ramify step`, `/tmp/x/ramify step` | build |
| Q7 | AC7 | high | `allow leaves substitution and redirection alone`: `$(…)`, backticks, `<(…)`, `> f`, `>> f`, `< f` | build |
| Q8 | AC8 | medium | `RAMIFY_AUTO_ALLOW=0 turns allow off` | build |
| Q9 | AC9 | medium | `allow ignores other commands`: `ls`, `git status` | build |
| Q10 | AC10 | medium | `allow takes the plugin's own bin/ramify` | build |
| Q11 | — | medium | `allow survives a command shlex cannot parse` (an open quote): no decision, exit 0 | build |
| Q12 | AC11 | low | Read `.claude-plugin/plugin.json`: the hook is under PreToolUse `Bash`. `hooks/codex.json` is unchanged | audit |
| Q13 | AC12 | low | Read the README against the hook: the allow list, the prompted list and `RAMIFY_AUTO_ALLOW` | audit |
| Q14 | AC1–AC10 | high | Read the parser for a command that runs anything but ramify, cd or a pipe filter and still gets `allow` | audit |
| Q14b | AC3 | high | `allow keeps pipe filters off files` and `allow takes filter flags and a grep pattern` (found by the Q14 read) | build |
| Q15 | AC1, AC4 | high | In a live Claude Code session with `--plugin-dir` on this worktree, `ramify step` runs with no prompt and `ramify delete x` still prompts | qa |

## Write set

```text
hooks/ramify-hook
.claude-plugin/plugin.json
test/smoke.sh
README.md
.ramify/work/allow-ramify-bash/*
!hooks/codex.json
!libexec/workflow.py
!libexec/native_guard.py
!app/*
```
