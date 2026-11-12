# Review: Ram Code edit tool

A separate reviewer agent read the branch diff and the record. It demonstrated each security finding with a scratch crate, and it edited nothing. Every finding below is fixed in `a81d1af` and `37a467d`, with a test that failed before its fix, unless the table says otherwise.

| # | Severity | Failure | Fix | Test |
|---|---|---|---|---|
| 1 | high | A dangling symlink passed the check, and the write followed it. `innocent -> .git/hooks/pre-commit` created a Git hook, and a link to `/private/tmp` wrote outside the worktree. Demonstrated. | `write::target` follows each part of the path with `symlink_metadata`, and refuses a link it cannot resolve, as the last part or a folder on the way. | `write_through_a_dangling_symlink_is_refused` |
| 2 | high | The temp file had a predictable name. A symlink planted there made an edit write an executable `.git/hooks/post-checkout`. Demonstrated. | A random temp name, opened with `create_new` and `O_NOFOLLOW`, with the mode set on that handle. | `planted_symlink_at_the_temp_path_is_harmless` |
| 3 | high | A hard link to `.git/config` let `write_file` set `core.fsmonitor`. Demonstrated. | A file with more than one hard link is refused, by both tools. | `hard_linked_target_is_refused` |
| — | medium | A new nested `.GIT` folder passed the check on a case-insensitive disk. | The `.git` check ignores case. | `git_folder_name_is_matched_without_case` |
| 4 | medium | A race between the check and the write. Reasoned. | Narrowed by fixes 1 and 2. See "Residual risk". | — |
| 5 | low | `write_file` wrote in place, so it was not atomic. | `write_file` uses the same `write::replace` as `edit_file`, and keeps the mode. | `write_file_is_atomic` |
| 6 | low | A mixed-ending edit put a bare LF into a CRLF region. Demonstrated. | Each replacement takes the ending of the line where its match starts. | `mixed_endings_follow_the_line_around_the_match` |
| 7 | low | A final newline in `new_string` made the line range one too long. | A final newline does not count as a line. | `line_range_for_trailing_newline` |
| C1 | conformance | The ambiguous-match error listed every line, and `replace_all` gave no lines. | At most 10 lines, then "and N more". `replace_all` reports the first and last line. | `ambiguous_error_lists_at_most_ten_lines`, `replace_all_reports_first_and_last_line` |
| S1 | simplicity | `resolve_writable` copied the probe loop, and `edit.rs` had a second `.git` check. | One `real_path` shared with `Tools::resolve`. The duplicate check is gone. | — |

## Not changed, accepted

- **8:** the rename loses ACLs, extended attributes and ownership, and it fails on a writable file in a read-only folder. This is rare in a worktree. The README describes the write rules.
- **9:** `aa` in `aaa` counts as one match, because the matches do not overlap. That is how `str::match_indices` counts, and the output says what changed.

## Residual risk

A process outside the task's process group could replace a folder on the path with a symlink between the check and the final `rename`, because `rename` resolves the folder path again. Closing that needs `openat` and `renameat` on a held folder handle, which `std` does not offer. The final file and the temp file can no longer be swapped. The attacker would first need a process that escaped the group kill.

## Checks after the fixes

- `cargo test`: 251 passed, 0 failed.
- `cargo clippy --all-targets`: no warnings.
- `ramify write-set .ramify/work/ram-code-edit-tool/write-set.txt`: the diff stays inside the write set.
