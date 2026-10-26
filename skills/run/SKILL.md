---
name: run
description: Hand the user a script to run or block — a command you may not run yourself, such as a write to production or a data import. It opens in the ramify run window with a Run and a Block button, and `ramify run wait` gives you the output. Use when a permission rule or the user blocks a risky command, or the user asks to run a script through ramify.
---

# ramify run

Write the commands to a script in your scratchpad, then hand it over:

Resolve the CLI with the [workflow contract](../workflow/SKILL.md#cli-and-runtime-tools).

```bash
ramify run ask <script> [--cwd <dir>] --note "<what it does, in one line>"
ramify run wait <id>     # use the workflow contract’s runtime wait tools
```

- Start the script with `set -euo pipefail`. Its input is `/dev/null`: it cannot ask a question.
- The script runs as the user, in `--cwd` (your directory by default), with the user's environment.
- Tell the user in one line that the script waits on them. Read the wait process’s completion output with the runtime tools in the workflow contract.
- `run wait` prints `ran: exit <n>` and the last 40 lines of output, or `blocked: <reason>`.
- Never edit the frozen copy in the runs folder. To change the script, ask again with a new one.
- `ramify run go` and `ramify run block` belong to the user. The plugin refuses them from you.
- The first use installs Electron, as `ramify app` does. If the window does not open, give the user the script path and the `ramify run go <id>` line.
