# Spec: allow ramify commands without a prompt

## Problem

An agent runs `ramify` many times in each workflow step: `ramify step`, `ramify step ask`,
`ramify card`. Claude Code asks the user for permission each time, unless a settings rule allows
it. The prompts interrupt the user for commands that only read state or record a step.

## Behaviour

A new PreToolUse hook, `ramify-hook allow`, reads each Bash command before it runs.

1. The hook splits the command into simple commands at `;`, `&&`, `||`, `|`, `&` and newlines.
2. It answers `permissionDecision: allow` only when every simple command is one of these:
   - `ramify <subcommand> …`, where the subcommand is on the allow list below. `RAMIFY_ROOT=` is
     the one environment prefix allowed. Any other `RAMIFY_*` variable can reach an `eval`.
   - `cd <dir>`, with an absolute path.
   - `head`, `tail`, `grep` or `wc` after a pipe, with fixed flags, counts and one `grep` pattern.
3. The command must be the bare `ramify` from `PATH`, or the plugin's own `bin/ramify`. A path
   to another copy (for example a worktree's `bin/ramify`) is not allowed: the agent can edit
   that file.
4. ramify sources the config of the tree it runs in. The hook's `cwd`, each `cd` target and a
   `RAMIFY_ROOT` value must be inside a worktree of the session's repo.
5. The command must hold none of `$`, backslash, backtick, `*`, `?`, `[`, `]`, `{`, `}`, `~`, `!`
   or `#`, not even inside quotes: bash expands what a token check cannot see. The only
   redirections accepted are the whole tokens `2>&1`, `>/dev/null` and `2>/dev/null`.
6. In every other case the hook prints nothing. Claude Code then applies its normal rules.
7. A `deny` from another hook (`decide-guard`, `pr-gate`, `behind`, `human-gate`) still wins.
   Claude Code applies deny before allow.
8. `RAMIFY_AUTO_ALLOW=0` in `.ramify.conf` turns the hook off for that repo.

### Allow list

| Allowed | Why |
|---|---|
| `config`, `workflow`, `gates`, `runs`, `label`, `card`, `dash`, `status`, `ping`, `statusline`, `write-set`, `help`, `--help` | They read state and change nothing. |
| `step` (status, `done` without `--approved`, `na`, `gate`, `ask`, `wait`, `asked`, `ensure`), `lane` | They record the work order. The agent records these under the workflow contract. |
| `run ask`, `run wait`, `scratch`, `approvals`, `app` | They hand something to the human or open a window. |
| `new`, `up`, `down`, `prune` without `--apply` | They cut a worktree or start and stop this worktree's own stack. A dry run changes nothing. |

### Still prompted

| Command | Why |
|---|---|
| `step done … --approved` | It records a human gate. The user must see it. |
| `approve`, `reject`, `run go`, `run block` | They belong to the human. `decide-guard` denies them. |
| `delete`, `complete`, `prune --apply`, `cleanup`, `shared-down` | They remove worktrees or branches, or stop processes that other sessions use. |
| `setup`, `jira`, `ticket`, `menu` | They write a config, store a credential, write to Jira, or need a terminal. |

## Acceptance criteria

- AC1: `ramify step` gets `allow`.
- AC2: `cd <worktree> && RAMIFY_ROOT=<worktree> ramify step ask spec` gets `allow`. A `cd`, a
  `RAMIFY_ROOT` or a `cwd` outside the repo's worktrees gets no decision.
- AC3: `ramify gates --json | head -5` and `ramify card 2>&1` get `allow`.
- AC4: Each command in "Still prompted" gets no decision.
- AC5: `ramify step && rm -rf x` gets no decision.
- AC6: `PATH=/tmp ramify step`, `./bin/ramify step` and `/tmp/x/ramify step` get no decision.
- AC7: `ramify step $(rm x)`, `` ramify step `rm x` ``, `ramify config > f`, a `#` comment, a
  variable, a glob, a brace, a tilde and any `RAMIFY_*` prefix but `RAMIFY_ROOT` get no decision.
- AC8: `ramify step` with `RAMIFY_AUTO_ALLOW=0` in the repo config gets no decision.
- AC9: A Bash call that is not about ramify (`ls`, `git status`) gets no decision.
- AC10: The plugin's own `bin/ramify` path, as the skills print it, gets `allow`.
- AC11: The hook is in `.claude-plugin/plugin.json` for Claude Code. `hooks/codex.json` does
  not change.
- AC12: The README lists the hook, the allow list and `RAMIFY_AUTO_ALLOW`.

## Out of scope

- The `human-gate` guard that blocks every Bash call while a gate waits. Another session works on
  it in `native-pipeline-gates`.
- Codex permissions.
- Other read-only tools after a pipe (`sed`, `awk`, `jq`).

## Open questions

- Is the allow list right? `new`, `up` and `down` start or stop processes. They act only on the
  agent's own worktree.

## Lane

Standard, forced by the `permissions` tag.
