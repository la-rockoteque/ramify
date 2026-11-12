# Audit: Ram Code reads AGENTS.md and CLAUDE.md

The diff is `git diff $(git merge-base HEAD origin/main)`: `src/instructions.rs` (new),
`src/agent.rs`, `src/app.rs`, `src/lib.rs`, `tests/qa_instructions.rs` (new), `AGENTS.md`
(new), `CLAUDE.md` (new) and `README.md`.

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `instructions::load` reads `~/.ram/AGENTS.md`, then `~/.claude/CLAUDE.md`, first. Test `user_files_come_first`. |
| AC2 | met | `PROJECT_FILES` gives the order in each folder. Test `project_files_come_in_order` writes the files in the reverse order. |
| AC3 | met | `folders` walks from the git top level down to the task folder. Test `a_parent_folder_comes_first`. |
| AC4 | met | `Reader::read` keeps the real path of each file read, and skips a path it saw. Test `a_linked_file_is_read_once`. |
| AC5 | met | `Reader::expand` skips fenced blocks and the odd parts between backticks. `imports` resolves `~/` and relative paths. Tests `an_import_brings_the_file_in`, `a_home_import_reads_from_home`. |
| AC6 | met | The same `seen` list stops a loop. Test `an_import_loop_ends`. |
| AC7 | met | `imports` keeps `@word` when no file resolves. Test `a_missing_import_stays_as_text`. |
| AC8 | met | `cut` with `FILE_LIMIT` per file, and `TOTAL_LIMIT` in `system_prompt`. A file that is not UTF-8 gives a note, which `run_turn_hooked` sends as a transcript line. Tests `a_large_file_is_cut`, `all_files_stop_at_128_kib`, `a_file_that_is_not_utf8_is_skipped`. |
| AC9 | met | `system_prompt` returns the base prompt when no section exists. Test `no_file_keeps_the_prompt`. |
| AC10 | met | The request's `system` goes through `openai::to_chat` unchanged. Test `an_openai_provider_gets_the_instructions`. |
| AC11 | met | `/instructions` in `App::handle_command` pushes `Instructions::listing`, with imported files marked `↳`. Test `the_instructions_command_lists_the_files`. |
| AC12 | met | `AGENTS.md` and `CLAUDE.md` (`@AGENTS.md`) at the root. Test `the_ramify_repo_reads_its_agents_md_once`. |
| AC13 | met | README section "Instructions: AGENTS.md and CLAUDE.md". |

## QA cases settled by the audit

| Case | Verdict | Evidence |
|---|---|---|
| Q17 | pass | The 15 paths that `AGENTS.md` names exist. `cargo test` passes in the root and in `cli/`, `cargo clippy --all-targets` reports nothing in either, and `bin/ramify write-set` runs. `AGENTS.md` names the two slow or fragile checks (`test/smoke.sh`, `test/skills.py` with PyYAML) as such. |
| Q18 | pass | README section "Instructions: AGENTS.md and CLAUDE.md": the files, the order, the imports, the limits, `/instructions` and `RAM_CODE_INSTRUCTIONS_HOME`. |

## Notes

- The instructions are read once per task, in the worker, so an edit to a file reaches the next
  task.
- Write set: the diff stays inside the plan's write set.
