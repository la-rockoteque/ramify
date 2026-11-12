# QA results: Ram Code edit tool

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `edit_replaces_one_match_and_nothing_else` |
| Q2 | pass | `edit_refuses_an_ambiguous_match` |
| Q3 | pass | `edit_replace_all_reports_the_count` |
| Q4 | pass | `edit_reports_no_match` |
| Q5 | pass | `edit_refuses_empty_or_identical_strings` |
| Q6 | pass | `edit_keeps_crlf_endings` |
| Q7 | pass | `edit_keeps_the_trailing_newline_state` |
| Q8 | pass after a fix | `edit_stays_in_the_worktree`, `file_tools_refuse_git_metadata_through_a_symlink`, `write_file_refuses_git_metadata` |
| Q9 | pass | `edit_refuses_missing_binary_and_large_files` |
| Q10 | pass | `edit_keeps_the_executable_bit` |
| Q11 | pass | `edit_output_is_one_line_with_the_range` |
| Q12 | pass | `both_formats_get_edit_file` |
| Q13 | pass | `edit_writes_atomically` |
| Q14 | waiting on the human | Needs a provider key. |

Yield: 14 cases. 13 ran and passed, 0 failed. The audit fixed 3 gaps. Q14 waits on a live run.
