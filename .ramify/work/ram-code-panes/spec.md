# Ram Code panes: sessions, agents, artifacts

## Problem

The panel shows the dashboard, approvals, scratch and run. The human also needs the sessions
that are running on this machine, the work each one started, and the work-order documents. Those
are the next three panes.

## Behaviour

The panel gains three panes, after the existing four, switched with `5`, `6` and `7` (and `[`/`]`
cycle all seven):

- **Sessions** reads `$STATE_DIR/sessions/*.root`. Each row shows the session id, its name from
  the matching `.name` file, its root and its branch. A session's colour comes from its name
  through the same MD5 palette as `ramify label`. The selected row is drawn in that colour, and
  so is the panel border in this pane. `Enter` opens that session's root in the chat: the panel
  selects the worktree and the transcript starts a new conversation for it.
- **Agents** lists the tagged process groups of the selected session's worktree (the services
  an agent started): worktree, service name and process group. The empty state names
  `ramify up`.
- **Artifacts** finds the work order of the selected worktree by branch and lists the documents
  that exist beside it: `spec.md`, `qa-plan.md`, `audit.md`, `qa-results.md`, `review.md`.
  The selected document's first 20 lines are shown under its row. The empty state names
  `ramify new` (or `ramify spec`).

The panel keeps its 40-column width. The command `--panel sessions|agents|artifacts` opens the
panel on that pane.

## Acceptance criteria

AC1. Given sessions in the state directory, then the Sessions pane lists each one with its name,
root and branch, and its colour.

AC2. Given a selected session, then its colour is used for that row and for the panel border.

AC3. Given a session, when the user presses `Enter`, then the chat's root becomes that session's
root and the project reloads for it.

AC4. Given the selected worktree, then the Agents pane lists its tagged process groups; without
any, it names `ramify up`.

AC5. Given the selected worktree, then the Artifacts pane lists the work-order documents that
exist and shows the selected one's first lines; without a work order it names `ramify new`.

AC6. The four existing panes and the chat keys keep working.

## Out of scope

- Collapsible session sections and opening one collapsing the others.
- Stopping or signalling agents from the panel.
- Editing artifacts from the panel.
- Pointing `ramify approvals|scratch|run` at the panel; the Electron windows stay for Claude
  Code and Codex.

## Lane

Standard. It adds panes to an existing TUI and reads existing state.
