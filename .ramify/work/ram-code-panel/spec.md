# Ram Code side panel

## Problem

The Electron app opens four windows: the dashboard, approvals, scratch and run. They need a
browser and a separate process. Ram Code already owns a terminal interface beside the worktrees.
The same views should live there, in a side panel next to the chat.

## Behaviour

Ram Code gets a right-hand side panel, 40 columns wide, closed by default. `Ctrl+P` toggles it.
The panel has four panes, switched with `1`–`4` or `[` and `]`:

- **Dashboard** reads `ramify dash --json` and lists every project, worktree, slot and service
  state. It refreshes when opened and every five seconds.
- **Approvals** reads `ramify gates --json`. Each row shows project, worktree, step and the
  files. `a` approves the selected gate, `r` rejects it after a note prompt.
- **Scratch** lists the items in the scratch directory. The selected item's text is shown.
  `c` copies it with `pbcopy`, `d` deletes it.
- **Run** reads `ramify runs --json`. `g` runs the selected pending script, `b` blocks it after
  a note prompt, and the pane shows the output when it finishes.

`Tab` keeps opening the worktree picker. `Esc` closes the panel before it leaves the chat.
`ram-code --panel approvals` (or `scratch`, `run`, `dashboard`) starts with the panel open on
that pane. Without state the panes show an empty line that names the command which creates it.

All Ramify calls go through the existing runner, with the selected worktree as root. The panel
does not change the chat.

## Acceptance criteria

AC1. Given the chat, when the user presses `Ctrl+P`, then the panel opens on the dashboard and
`Ctrl+P` or `Esc` closes it again.

AC2. Given the dashboard pane, then it lists the project and its worktrees with their slot and
service state, and refreshes without user input.

AC3. Given the approvals pane, then it lists every open gate. `a` runs `ramify approve <step>`
in that worktree and refreshes. `r` asks for a note, runs `ramify reject <step> --note <note>`,
and refreshes.

AC4. Given the scratch pane, then it lists the items and shows the selected text. `c` copies
the text. `d` removes the item.

AC5. Given the run pane, then it lists the runs. `g` runs the selected pending script, `b`
blocks it with a note, and the pane shows the output after the run.

AC6. Given `ram-code --panel <pane>`, then the panel starts open on that pane.

AC7. Given no gates, scratch items or runs, then the pane shows an empty state that names the
command which creates them.

## Out of scope

- Removing the Electron windows; they stay until the panel matches them.
- Pointing `ramify approvals|scratch|run` at Ram Code.
- Sessions, agents and work-order artifacts in the panel; those come next.
- Editing files from the panel.

## Lane

Standard. It adds a screen to an existing TUI and calls existing CLI commands.
