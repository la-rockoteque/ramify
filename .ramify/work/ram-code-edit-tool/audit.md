# Audit: Ram Code edit tool

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `src/edit.rs` `edit_file`, `write_over` | `edit_replaces_one_match_and_nothing_else`, `edit_writes_atomically` |
| AC2 | met | `src/edit.rs` match count and `line_of` | `edit_refuses_an_ambiguous_match` |
| AC3 | met | `src/edit.rs` `replace_all` | `edit_replace_all_reports_the_count` |
| AC4 | met | `src/edit.rs` no-match error | `edit_reports_no_match` |
| AC5 | met | `src/edit.rs` empty and identical checks | `edit_refuses_empty_or_identical_strings` |
| AC6 | met | `src/edit.rs` `to_crlf` for a CRLF file | `edit_keeps_crlf_endings` |
| AC7 | met | `src/edit.rs`: the text is replaced in place, so the final newline state stays | `edit_keeps_the_trailing_newline_state` |
| AC8 | met after a fix | `src/tools.rs` `resolve_writable` | `edit_stays_in_the_worktree`, `file_tools_refuse_git_metadata_through_a_symlink`, `write_file_refuses_git_metadata` |
| AC9 | met | `src/search.rs` `read_utf8` | `edit_refuses_missing_binary_and_large_files` |
| AC10 | met | `src/edit.rs` `write_over` keeps the mode | `edit_keeps_the_executable_bit` |
| AC11 | met | `src/edit.rs` output | `edit_output_is_one_line_with_the_range` |
| AC12 | met | `src/tools.rs` definitions, `src/agent.rs` `SYSTEM_PROMPT` | `both_formats_get_edit_file` |

## Fixed during the audit (`e6c3f10`)

1. **Git metadata through a symlink, and in `write_file`.** `edit_file` checked `.git` only in the path the model gave, so a symlink to `.git` passed. `write_file` had no check at all, and that hole is on `main` today. In a linked worktree, `.git` is a file that names the Git folder, so `write_file .git` could point Git at a folder the model controls. Either route lets the model plant `core.fsmonitor` or a hook, and Git runs it later outside the sandbox: the H3 class from #62, through the file tools. Both tools now go through `resolve_writable`, which resolves the real target and refuses any `.git` component under the worktree. Tests: `file_tools_refuse_git_metadata_through_a_symlink`, `write_file_refuses_git_metadata`. Both failed before the fix.
2. **Mixed line endings.** Any CRLF in a file turned the strings into CRLF, so an edit on an LF line of a mixed file failed. The spec says a mixed file is matched as it is. A file with only CRLF still converts. A mixed file matches as it is first, and with CRLF only if that fails. Test: `edit_matches_a_mixed_file_as_is`.
3. **Editing a symlink.** The rename replaced the link with a plain file. Writes now go to the real target, so the link stays. Test: `edit_through_a_symlink_keeps_the_link`.

## Notes

- Q5, Q8 and Q9 first passed through the "unknown tool" error. The build confirmed that each now passes on the tool's own refusal.
- The rename gives the file a new inode, so a hard link to it keeps the old content. Ownership and extended attributes are not copied. Both are rare in a worktree.

## Checks

`cargo test`: 242 passed, 0 failed. `cargo clippy --all-targets`: no warnings. `ramify write-set .ramify/work/ram-code-edit-tool/write-set.txt`: the diff stays inside the write set.
