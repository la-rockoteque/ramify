# QA results: Codex handoff

## Automated checks

- `asdf exec python3 test/codex.py`: 6 tests pass. The tests cover unfinished build files, pending gates, rejection notes, plugin metadata, decision guards, worktree output, compaction, and session cleanup.
- `PATH=/Users/rocko/.asdf/shims:$PATH TERM=xterm-256color bash test/smoke.sh`: the final clean run passes 187 cases; 2 existing colour cases fail.
- The same two colour cases fail on unchanged `origin/main` (`f312e28`). A focused baseline run reproduced both failures.
- `git diff --check`: pass.
- `bin/ramify write-set .ramify/work/codex-handoff/write-set.txt`: pass.
- The skill-creator validator accepts `skills/resume`.
- CodeScene rates `test/codex.py` at 10/10. It cannot analyse the extensionless Bash hook.

## Case record

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | Installed this branch with `codex plugin add` through a transient local marketplace override. A fresh Codex session discovered `ramify:resume` from the installed cache before opening repo files. |
| Q2 | pass | `test_resume_preserves_unfinished_build` preserves changed and untracked files. A fresh Codex reviewer read the installed skill and recovered the existing work order without edits. It was restricted to review, so it did not implement the next step. |
| Q3 | pass | The same test confirms that build follows the completed spec and QA plan. |
| Q4 | pass | `test_resume_preserves_pending_approval` keeps the request and order byte-for-byte intact. |
| Q5 | pass | Shared skills use CLI resolution and the workflow contract’s runtime wait tools. Claude-only labels apply only to Claude. |
| Q6 | pass | Explicit installed Claude and Codex CLIs produced identical config, state-directory, registry, pending-approval, and statusline stack/workflow reports for one isolated worktree. Its order stayed unchanged. This checks the installed CLIs; it does not simulate a Claude chat session. |
| Q7 | pass | Codex uses the documented Bash matcher, common event payloads, PLUGIN_ROOT, and hook response format. README links the official packaging and hook documentation. |
| Q8 | baseline failures | All Claude hook cases pass. The only smoke failures are the two unchanged dashboard colour cases reproduced on main. |
| Q9 | pass | `test_no_order` verifies silent startup without an order. The resume skill reports the missing order and asks before starting one. |
| Q10 | pass | The pending-approval test rejects its fixture request and confirms that the note survives another resume. |
| Q11 | pass | The fresh Codex reviewer discovered the installed skill and ran bundled `step` and `step asked --json` commands in a read-only sandbox without permission errors. No hook trust was bypassed. |
| Q12 | pass | `test_resume_preserves_unfinished_build` |
| Q13 | pass | `test_resume_preserves_pending_approval` |
| Q14 | pass | `test_codex_package`, plus the actual Codex CLI package listing |
| Q15 | pass | `test_no_order` |
| Q16 | pass | `test_codex_decision_guard` |
| Q17 | pass | `test_codex_worktree_output`, including compaction and session-end cleanup |

Yield: 6 new regression tests pass. The final smoke suite passes 187 cases and has 2 baseline failures. Fresh-session discovery and read-only recovery pass.

## Audit against the acceptance criteria

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | Codex manifest, marketplace, documented GitHub/local installation, and fresh-session skill discovery. |
| AC2 | met | Existing work order and Git worktree remain the handoff point; no new order or branch is created by resume. |
| AC3 | met | Installed resume skill requires status, both diffs, earlier outputs, and relevant untracked files before edits. |
| AC4 | met | Build-resume fixture keeps the completed record intact; fresh-session recovery identifies build as next. |
| AC5 | met | Pending request and rejection-note fixtures retain their records; the shared decision guard still denies agent decisions. |
| AC6 | met | Shared skills resolve their own bundled CLI and describe both runtimes’ wait tools. |
| AC7 | met | Installed CLI comparison proves shared resolution without a migration. |
| AC8 | met | Hook configuration follows official event, Bash matcher, payload, output, and plugin-root contracts. Manual recovery is documented and exercised. |
| AC9 | met, baseline caveat | All existing Claude hook cases pass. The only remaining smoke failures also fail on unchanged main. |

## Review fixes

The initial reviewer found that recovery inspection attempted registry writes and that Q6 overstated its evidence.
Workflow inspection now skips runtime writes and ancestor process scans. The regression test checks that resume creates no state or registry.
Shared skills now prefer their bundled CLI over a possibly older CLI on PATH.
The explicit installed-CLI comparison supplies Q6 evidence.
A fresh follow-up reviewer found no correctness regressions in these fixes.

## Limits

The two existing dashboard colour failures remain outside this branch’s scope.
Hook trust still belongs to the user; the QA sessions did not bypass it.
The GUI launcher itself is unchanged. Direct launch resolved the approval-window issue during this session.
