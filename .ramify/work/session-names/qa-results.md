# Session names QA results

## Audit

Each acceptance criterion, with the code and the test that meet it. Code is in
`hooks/ramify-hook` unless noted. Tests are `check` lines in `test/smoke.sh`.

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `session_name`, `name_words` read `assets/names/trees.txt` when the setting is empty | `session-start writes a name from trees`, `the name file holds one line` |
| AC2 | met | `claim_name` creates `held/<name>` with noclobber | `two sessions get two names` |
| AC3 | met | `session_name` tries every word with `""`, then `-2` up to `-9` | `a full theme adds -2` |
| AC4 | met | `name_words` takes `assets/names/<value>.txt` | `RAMIFY_SESSION_NAMES picks a built-in theme` |
| AC5 | met | `name_words` splits a value with spaces | `RAMIFY_SESSION_NAMES takes a word list` |
| AC6 | met | `session_name` returns early on `off`; `session_title` prints nothing without a name in the primary | `RAMIFY_SESSION_NAMES=off writes no name`, and the four title checks from `main` run with `off` |
| AC7 | met | `session_title` prints `<slug> · <name>` in a linked worktree | `session-start titles a worktree slug · name` |
| AC8 | met | `session_title` prints `<name>` in the primary | `session-start titles the primary with the name` |
| AC9 | met | `on_session_start` runs `session_name` before `session_title` checks `session_title` | `a custom title keeps the name file` |
| AC10 | met | `release_name` runs first in `on_session_end`, before the `prompt_input_exit` return | `session-end releases the name`, `prompt_input_exit releases the name too` |
| AC11 | met | `title_name` reads the name from `session_title` when it is a theme word; `session_name` claims it on `source: resume` | `resume takes the name from a worktree title`, `resume takes the name from a primary title`, `resume takes a new name when the title's name is held`, `resume does not take a slug title as a name` |
| AC12 | met | `session_name` needs `configured` | `no config, no name` |
| AC13 | met | `libexec/statusline.sh` reads `<session_id>.name` | `statusline --json has the name`, `statusline --json name is null without one`, `the plain status line starts with the name` |
| AC14 | met | `README.md`: the config table, the Session names section, the status line section and the SessionStart hook line | Q21 below |

## Audit cases

| Case | Verdict | Evidence |
|---|---|---|
| Q21 | pass | `README.md` names `RAMIFY_SESSION_NAMES`, `trees` and `birds`, `assets/names/<theme>.txt`, `${RAMIFY_STATE_DIR:-/tmp/ramify}/sessions/<session_id>.name`, the release on every end, the resume from the title and both title formats. Each matches `hooks/ramify-hook`. |
| Q22 | pass after a fix | The hook runs with `set -uo pipefail` and no `-e`. Every failure path in `session_name` returns 0, and `an unwritable state dir names nothing and exits 0` covers it. Every path is quoted. `/bin/bash -n` passes on the three changed scripts. The audit found one gap: a theme with no usable word claimed `-2`. `session_name` now skips an empty word. Test: `a theme without a usable word names nothing`. |

## Write set

`ramify write-set` on the plan's set: the diff stays inside it.

## Gaps left on purpose

- `held/<name>` files from a session that crashed stay until `/tmp` is cleared. The spec puts
  clean-up out of scope. The `-2` suffix keeps names unique in the meantime.

## QA

| Case | Result | Evidence |
|---|---|---|
| Q1–Q20 | pass | `bash test/smoke.sh`: smoke: all passed, every check named in the plan included, plus `a theme without a usable word names nothing` |
| Q21, Q22 | pass | the audit cases above |
| Q23 | pass | Two live sessions with this branch's hook: `sessions/330faad0-….name` holds `palm`, `sessions/433e2e0c-….name` holds `teak`, and `held/` holds both. The user confirmed the titles. |
| Q24 | pending | the first live run found two gaps, below. A rerun with the fixed hook is due |

## Found in live QA

- Q24, first run: `/exit` ends a session with `prompt_input_exit`, and the hook kept the name
  held. Almost every session ends this way, so names leaked. Fix: `release_name` runs on every
  end. Test: `prompt_input_exit releases the name too`.
- Q24, first run: a resumed session got a new `session_id` (`5bd3fe84…`) and a new name (`ash`),
  while its title kept the old one. The spec assumed the `session_id` stays. Fix: a resume takes
  the name from its title. The user approved the spec change in the chat. Tests: the four
  `resume …` checks.
