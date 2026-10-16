---
name: run
description: Hand the user a script to run or block — a command you may not run yourself, such as a write to production or a data import. It opens in the ramify run window with a Run and a Block button, and `ramify run wait` gives you the output. Use when a permission rule or the user blocks a risky command, or the user asks to run a script through ramify.
---

# ramify run

Write the commands to a script in your scratchpad, then hand it over:

```bash
"${CLAUDE_PLUGIN_ROOT}/bin/ramify" run ask <script> [--cwd <dir>] --note "<what it does, in one line>"
"${CLAUDE_PLUGIN_ROOT}/bin/ramify" run wait <id>     # with run_in_background
```

- Start the script with `set -euo pipefail`. Its input is `/dev/null`: it cannot ask a question.
- The script runs as the user, in `--cwd` (your directory by default), with the user's environment.
- Tell the user in one line that the script waits on them. Do not poll `run wait`: its exit wakes you.
- `run wait` prints `ran: exit <n>` and the last 40 lines of output, or `blocked: <reason>`.
- Never edit the frozen copy in the runs folder. To change the script, ask again with a new one.
- `ramify run go` and `ramify run block` belong to the user. The plugin refuses them from you.
- The first use installs Electron, as `ramify app` does. If the window does not open, give the user the script path and the `ramify run go <id>` line.
