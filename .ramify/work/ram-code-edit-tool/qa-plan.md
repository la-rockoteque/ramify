# QA plan: Ram Code edit tool

The repo declares no services. The build settles the automated cases with `cargo test`, in `tests/qa_edit.rs`. Each test builds a small worktree in a temporary folder.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `edit_replaces_one_match_and_nothing_else`: the bytes before and after the match are unchanged. | build |
| Q2 | AC2 | high | `edit_refuses_an_ambiguous_match`: three matches without `replace_all` give an error with `3` and the three line numbers. The file's bytes are unchanged. | build |
| Q3 | AC3 | medium | `edit_replace_all_reports_the_count`: three matches with `replace_all` all change, and the output says 3. | build |
| Q4 | AC4 | medium | `edit_reports_no_match`: the error names `read_file` or `grep`. The file is unchanged. | build |
| Q5 | AC5 | medium | `edit_refuses_empty_or_identical_strings` | build |
| Q6 | AC6 | high | `edit_keeps_crlf_endings`: an LF `old_string` and `new_string` on a CRLF file. Every line still ends in CRLF, including the edited lines. | build |
| Q7 | AC7 | medium | `edit_keeps_the_trailing_newline_state`: an edit on the last line of a file with and without a final newline. | build |
| Q8 | AC8 | high | `edit_stays_in_the_worktree`: `../x`, an absolute path, a symlink out, and `.git/config` each give an error, and nothing outside changes. | build |
| Q9 | AC9 | medium | `edit_refuses_missing_binary_and_large_files`: a missing file, a file with NUL bytes, invalid UTF-8, and a file over 10 MB. | build |
| Q10 | AC10 | medium | `edit_keeps_the_executable_bit` | build |
| Q11 | AC11 | low | `edit_output_is_one_line_with_the_range`: the output has the new line range and none of the file's text. | build |
| Q12 | AC12 | medium | `both_formats_get_edit_file`: both request formats list `edit_file`. The system prompt and the `write_file` description name `edit_file`. | build |
| Q13 | AC1 | medium | `edit_writes_atomically`: after an edit, no temporary file is left in the folder, and the inode changes (rename) while the content is right. | build |
| Q14 | AC1–AC11 | medium | Live: ask Ram Code to rename a local variable in one function of this repo. The transcript shows `edit_file`, not `write_file`, and `git diff` shows only those lines. | qa |

## Write set

May change: `src/**`, `tests/**`, `README.md`, `.ramify/work/ram-code-edit-tool/**`.

Must not change: `Cargo.toml`, `Cargo.lock`, `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`, `.codex-plugin/**`, other `.ramify/work/**` folders.
