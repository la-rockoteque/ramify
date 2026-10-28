# Spec: session names

## Problem

Several agent sessions often run on one machine at the same time. Two sessions in the same
worktree have the same title, and a session in the primary checkout has no title. The user
cannot tell them apart in the session list or in the status line.

## Behaviour

ramify gives each session a short **session name**, such as `birch`. The name comes from a
**name theme**: a list of words. The default theme is `trees`.

1. The `session-start` hook fires in a repo with a ramify config.
2. When the session has no name yet, the hook picks one word from the theme at random. It skips
   every name that a live session on the machine already holds.
3. When every word of the theme is held, the hook adds a number: `birch-2`.
4. The hook writes the name to `${RAMIFY_STATE_DIR:-/tmp/ramify}/sessions/<session_id>.name`,
   one line, no other content. This file is the contract with other tools, such as barbarbar.
5. The hook sets the Claude Code session title, under the rules that exist today (startup,
   resume and fork only; a title the human set wins):
   - In a linked worktree: `<slug> · <name>`, for example `tm-127-pagination · birch`.
   - In the primary checkout: `<name>`.
6. The `session-end` hook releases the name on every end, `prompt_input_exit` included. It
   removes the name file and the hold, so another session can take the name.
7. A resumed session may not find its name file. When its title is `<slug> · <name>` or `<name>`,
   and `<name>` is a word of the theme (with or without a `-N` suffix) that no live session
   holds, the hook gives it that name again. The name file follows the title the user sees.
   Any other title, such as one the user set, gets a new name.
8. `ramify statusline --json` adds a `"name"` field (a string, or null). The plain line starts
   with the name: `𖣂 birch · tm-127-pagination ⎇ feat/tm-127-pagination`.

### The theme setting

`RAMIFY_SESSION_NAMES` in `.ramify.conf`:

| Value | Effect |
|---|---|
| unset or empty | the `trees` theme |
| a built-in theme name: `trees` or `birds` | that theme |
| several words, separated by spaces | that list is the theme |
| `off` | no name, no name file; titles stay as they are today |

A built-in theme is a file, `assets/names/<theme>.txt`, one word per line. Adding a theme is
adding a file. An unknown single word is a theme of one word, so a typo still gives a name;
`ramify config` warns about it.

### States

- No ramify config in the repo: no name. The hooks stay silent, as today.
- Codex (`RAMIFY_AGENT=codex`): the name file is written. No title is set, as today.
- The state directory is not writable: no name, no error. The hook never fails a turn.

## Acceptance criteria

- AC1: Given a configured repo without `RAMIFY_SESSION_NAMES`, when a session starts, then
  `sessions/<session_id>.name` holds one word from `assets/names/trees.txt`.
- AC2: Given two sessions that start one after the other, then their names are different.
- AC3: Given a theme of two words and two live sessions, when a third session starts, then its
  name is one of the two words with `-2` appended.
- AC4: Given `RAMIFY_SESSION_NAMES=birds`, then the name comes from `assets/names/birds.txt`.
- AC5: Given `RAMIFY_SESSION_NAMES="ash elm"`, then the name is `ash` or `elm`.
- AC6: Given `RAMIFY_SESSION_NAMES=off`, then no name file is written and the title rules are
  the same as on `main`.
- AC7: Given a linked worktree and `source: startup`, then `sessionTitle` is
  `<slug> · <name>`.
- AC8: Given the primary checkout and `source: startup`, then `sessionTitle` is `<name>`.
- AC9: Given a `session_title` that is not empty, then the hook sets no `sessionTitle`, and the
  name file is still written.
- AC10: Given a session that ends with any reason, `prompt_input_exit` included, then its name
  file and its hold are gone, and another session can take the name.
- AC11: Given a resumed session whose title is `<slug> · <name>` or `<name>`, and nobody holds
  `<name>`, then it gets `<name>` back. When a live session holds it, or the title has no theme
  word (a slug alone, a title the user set), then it gets a new name.
- AC12: Given a repo without a ramify config, then no name file is written.
- AC13: `ramify statusline --json` prints `"name": "<name>"` for a named session and
  `"name": null` otherwise. The plain line starts with the name when there is one.
- AC14: The README documents `RAMIFY_SESSION_NAMES`, the themes and the name file path.

## Out of scope

- The barbarbar segment. It reads the name file and is a change in the barbarbar repo, done
  after this branch. It needs only the path in step 4.
- A command to rename a session. `/rename` in Claude Code changes the title; the name file
  keeps the name ramify gave.
- Clean-up of the hold that a crashed session leaves behind. `ramify prune` can take that
  later.
- Names in the desktop app.

## Open questions

None. The user chose the built-in themes: `trees` (the default) and `birds`. barbarbar belongs
under `la-rockoteque`. It is not on GitHub yet, and the local repo has no remote.

## Lane

Standard. The name file is a new contract that another tool reads, so the lighter lane does
not apply.
