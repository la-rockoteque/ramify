# Ram Code sessions: collapsible groups and agent stop

## Problem

The Sessions pane lists every session in one flat column. With several sessions the list is
noise. The Agents pane lists the services a worktree started but cannot stop them.

## Behaviour

The Sessions pane groups sessions by project: the basename of the primary checkout of the
session's root. Each group is one header row with the project name, the number of sessions and
a fold marker (`▸` closed, `▾` open). Exactly one group is open at a time; it opens on the group
of the current worktree, else the first group.

`j` and `k` move over the visible rows: every header, and the sessions of the open group.
`Enter` or `Space` on a header opens it and closes every other group. `Enter` on a session opens
that session's root in the chat, as today.

In the Agents pane, `x` stops the selected worktree's stack by running `down` with that
worktree as root. The result appears in the panel message and the list refreshes. The Electron
windows stay for Claude Code and Codex.

## Acceptance criteria

AC1. Given sessions under several projects, then the pane lists a header per project with its
session count, and only the open group's sessions.

AC2. Given a closed group, when the user presses `Enter` or `Space` on its header, then it opens
and every other group closes.

AC3. Given an open group, when the user presses `Enter` on a session, then the chat's root
becomes that session's root.

AC4. Given a service in the Agents pane, when the user presses `x`, then `down` runs with the
selected worktree as root and its output is shown.

AC5. The other panes and keys keep working, and the pane opens on the current worktree's group.

## Out of scope

- Remembering the open group between runs.
- Stopping one service rather than the whole worktree stack.
- The Electron windows.

## Lane

Standard. It changes how an existing pane lists data and adds one action that calls an existing
command.
