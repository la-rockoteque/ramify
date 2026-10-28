# Allow ramify commands QA results

## Audit

Code is in `hooks/ramify-hook`: `is_plain_ramify` (the parser) and `on_allow`. Tests are
`check` lines in `test/smoke.sh`.

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `ramify_ok`: `step` without an `--appr…` argument | `allow lets ramify step through` |
| AC2 | met | `simple_ok`: `cd` with one argument; `RAMIFY_*=` assignments are skipped | `allow takes cd and a RAMIFY_ prefix` |
| AC3 | met | `2>&1`, `2>/dev/null` and `>/dev/null` are removed before the check; `filter_ok` after a pipe | `allow takes a pipe to head and 2>&1`, `allow takes filter flags and a grep pattern` |
| AC4 | met | `ramify_ok` returns False for every subcommand outside the allow list | `allow leaves every prompted command alone` |
| AC5 | met | every simple command between separators must pass | `allow leaves a chain with another command alone` |
| AC6 | met | `simple_ok`: bare `ramify`, or an absolute path whose realpath is the plugin's `bin/ramify`; any other assignment fails | `allow leaves another ramify binary alone` |
| AC7 | met | a `<`, `>`, backtick or `$(` left after the three accepted redirections fails | `allow leaves substitution and redirection alone` |
| AC8 | met | `on_allow` reads `RAMIFY_AUTO_ALLOW` through `conf` | `RAMIFY_AUTO_ALLOW=0 turns allow off` |
| AC9 | met | `on_allow` returns at once when the input has no `ramify` | `allow ignores other commands` |
| AC10 | met | `realpath(c[0]) == realpath($RAMIFY)` | `allow takes the plugin's own bin/ramify` |
| AC11 | met | `.claude-plugin/plugin.json` has one `ramify-hook" allow` entry under PreToolUse `Bash` | Q12 |
| AC12 | met | README: the config table, the hook list and "Commands that run without a prompt" | Q13 |

## Audit cases

| Case | Verdict | Evidence |
|---|---|---|
| Q12 | pass | `.claude-plugin/plugin.json` holds the `allow` hook once, after `decide-guard`. `git diff origin/main -- hooks/codex.json` is empty. |
| Q13 | pass | README line 199 has `RAMIFY_AUTO_ALLOW`. The section at line 551 has the allow list, the prompted list, the binary rule, the redirection rule and the filter rule. Each matches `is_plain_ramify`. |
| Q14 | pass after fixes | I fed the hook 24 adversarial commands: newlines, `&`, `{ … }`, `eval`, `\|&`, substitution in quotes and in assignments, `LD_PRELOAD=`, `cd "$(…)"`, `#` comments and `--help` on a prompted command. Two gaps got `allow`. (1) A pipe filter read a file: `\| head -c 1 /etc/shadow`, `\| tail -f x`. `filter_ok` now takes flags and counts only, and one pattern for `grep` without `-f`, `-r`, `-R`, `-d` or `-D`. Tests: `allow keeps pipe filters off files`, `allow takes filter flags and a grep pattern`. (2) `--approved=1` and `--approve`. Both already fail in `workflow.py` (`unknown option`), but `ramify_ok` now refuses any argument that starts with `--appr`. Test: in `allow leaves every prompted command alone`. `ramify step #; rm x` gets `allow`, and that is correct: bash and shlex both read `#` as a comment. `ramify delete x --help` gets `allow`, and that is correct too: `bin/ramify` prints the help and exits before it runs any command. |

## Write set

`ramify write-set` on the plan's set: the diff stays inside it.
