# QA plan: Ram Code reads AGENTS.md and CLAUDE.md

The automated cases go in a new file, `tests/qa_instructions.rs`. Each test makes a git repo
and a home folder in a temp folder, and sets `RAM_CODE_INSTRUCTIONS_HOME` to that home folder.
A fake model records each request, so a test reads the `system` text that a task sent, as
`tests/qa_hooks.rs` does.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `user_files_come_first`: `~/.ram/AGENTS.md` and `~/.claude/CLAUDE.md`; the system text has both, `AGENTS.md` first, each after `Instructions from ~/…:` | build |
| Q2 | AC2 | high | `project_files_come_in_order`: the four project files; their texts come in the order `AGENTS.md`, `CLAUDE.md`, `.claude/CLAUDE.md`, `CLAUDE.local.md`, after the user's files | build |
| Q3 | AC3 | medium | `a_parent_folder_comes_first`: the task runs in `sub/` of the repo, with an `AGENTS.md` in each; the top-level text comes first | build |
| Q4 | AC4 | high | `a_linked_file_is_read_once`: `CLAUDE.md` is a symlink to `AGENTS.md`; the text is in the system text one time | build |
| Q5 | AC5 | high | `an_import_brings_the_file_in`: `CLAUDE.md` holds `see @docs/rules.md`; the rules text is in its place. `` `@docs/rules.md` `` in a code span and in a fenced block stays as written | build |
| Q6 | AC5 | low | `a_home_import_reads_from_home`: `@~/shared.md` reads the file in the test home | build |
| Q7 | AC6 | high | `an_import_loop_ends`: `a.md` imports `b.md`, which imports `a.md`; each text is there one time, and the task gets its request | build |
| Q8 | AC5 | low | `imports_stop_at_depth_five`: a chain of 7 files; the texts of the first 6 (the file and 5 levels) are there, and the 7th stays as `@path` | build |
| Q9 | AC7 | medium | `a_missing_import_stays_as_text`: `@nope.md` stays | build |
| Q10 | AC8 | medium | `a_large_file_is_cut`: a 40 KiB file; the system text has its first 32 KiB and `[cut at 32 KiB]` | build |
| Q11 | AC8 | low | `all_files_stop_at_128_kib`: five files of 30 KiB; the instruction part is at most 128 KiB and ends with the note | build |
| Q12 | AC8 | low | `a_file_that_is_not_utf8_is_skipped`: the other files are there, and one transcript line names the file | build |
| Q13 | AC9 | high | `no_file_keeps_the_prompt`: the system text equals Ram Code's own prompt | build |
| Q14 | AC10 | medium | `an_openai_provider_gets_the_instructions`: `openai::to_chat` of a request with the instructions gives a system message that holds them | build |
| Q15 | AC11 | medium | `the_instructions_command_lists_the_files`: an `App` on the repo; `/instructions` shows each path with its size, in order. On an empty repo it shows `no AGENTS.md or CLAUDE.md` | build |
| Q16 | AC12 | medium | `the_ramify_repo_reads_its_agents_md_once`: the repo's own folder; the system text holds the text of `AGENTS.md` one time | build |
| Q17 | AC12 | low | The repo's `AGENTS.md` is accurate: each command it names runs, and each path it names exists | audit |
| Q18 | AC13 | low | The README documents the files, the order, the imports, the limits and `/instructions` | audit |
| Q19 | AC1–AC9 | medium | Live: in the ramify worktree, `ram --task "Quote the first line of your instructions from AGENTS.md"` with a configured provider; the answer quotes it. Needs a provider: the user may waive it | qa |

## Write set

```
src/**
tests/qa_instructions.rs
README.md
AGENTS.md
CLAUDE.md
.ramify/work/ram-instructions/**
!cli/**
!hooks/**
!install.sh
!workflows.yml
```
