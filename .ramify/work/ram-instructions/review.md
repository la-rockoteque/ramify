# Review: Ram Code reads AGENTS.md and CLAUDE.md

A fresh subagent reviewed the branch diff against the spec and the QA record. It ran the tests,
clippy and a probe against `src/instructions.rs`. It reported 1 high, 3 medium and 5 low
findings. The fixes are below. The spec has a review amendment (10–14).

| # | Severity | Where | Failure | Fix | Status |
|---|---|---|---|---|---|
| H1 | high | `resolve`, `read` | A cloned repo's `CLAUDE.md` with `@~/.ssh/id_rsa`, `@/abs` or `@../x`, or a `CLAUDE.md` link to a file outside, sent that file to the model on every task. Claude Code asks first; Ram Code did not. | A project file and its imports must have a real path inside the git top level. `~/` and absolute imports work only in the user's own files. Each refusal keeps the text and adds a transcript line. Tests `a_project_cannot_import_outside_itself`, `a_linked_claude_md_outside_the_project_is_skipped`. README caution. | fixed |
| M1 | medium | `name` | A repo in the home folder was named `~/dev/repo/AGENTS.md`, not `AGENTS.md`. | The git top level comes first. Test `a_repo_under_home_is_named_from_its_top`. | fixed |
| M2 | medium | `top_file`, `system_prompt` | Imports took the importer's 32 KiB, and `/instructions` listed text that the model never got. | Each file is cut when it is read. The total keeps whole sections in order, cuts the one that crosses the limit, and leaves the rest out with a note. The listing says `cut at 32 KiB`, `cut at 128 KiB` or `left out: over 128 KiB`. Tests `an_import_is_cut_on_its_own`, `the_listing_shows_what_the_limits_did`. Keeping the nearest files last is a choice, with a `ponytail:` note. | fixed |
| M3 | medium | `tests/qa_plan.rs`, `qa_providers.rs`, `qa_hooks.rs` | The other tests read the developer's own `~/.ram/AGENTS.md` and `~/.claude/CLAUDE.md`. | Each of them points `RAM_CODE_INSTRUCTIONS_HOME` at a temp folder. | fixed |
| L1 | low | `home` | With no `HOME`, names read `~//…` and the user's files were read from the working folder. | `home()` gives None, and then only the project's files are read. Test `no_home_reads_only_the_project`. | fixed |
| L2 | low | tests | No test for the cut on a character boundary, for `AGENTS.md` with no imports, for an e-mail address, or for a linked worktree. | Tests `a_cut_keeps_whole_characters`, `agents_md_has_no_imports_and_an_email_is_no_import`, `a_linked_worktree_reads_its_own_files`. | fixed |
| L3 | low | `expand` | A ```` ``` ```` fence closed a `~~~` fence, and the other markdown limits were not written down. | A fence closes only on its own marker. Test `a_tilde_fence_closes_only_on_a_tilde_fence`. A `ponytail:` note names the other limits. | fixed |
| L4 | low | README | The README said "the files that Codex and Claude Code read", but Ram Code reads no file above the git top level. | The README says so. | fixed |
| L5 | low | `/instructions` | It ran `git` and read files on the interface thread. | It runs on a thread, and its lines reach the transcript through the slot that SessionStart uses. | fixed |

Also found during the fixes: the cut of the section that crosses 128 KiB put its note after the
limit. The note now counts inside it.

Checks after the fixes: `cargo test` passes (`tests/qa_instructions.rs` 26), and
`cargo clippy --all-targets` reports nothing.
