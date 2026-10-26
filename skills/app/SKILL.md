---
name: app
description: Open the ramify desktop dashboard, a window with every project's worktrees and QA stacks and buttons to start, stop, create, delete, prune and clean them up. Use when asked to open the ramify app, the visual dashboard or the GUI.
---

# ramify app

Resolve the CLI with the [workflow contract](../workflow/SKILL.md#cli-and-runtime-tools).

```bash
ramify app
```

- The first run installs Electron into `~/.cache/ramify/electron`. It needs Node.js ≥ 22.12 and
  npm, takes up to a minute, and happens once per Electron version.
- The window runs detached. Verify that it appears before reporting success.
- If the launcher reports success without a window, capture the app’s startup output before retrying.
- On an install failure, show the error and the last lines of the install log it names.

The window reads the same state as `ramify dash`. Each button runs a `ramify` command, and its
output goes to the log panel at the bottom of the window.
