# QA results: Ram Code search tools

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `grep_skips_ignored_files` |
| Q2 | pass | `grep_skips_the_git_folder` |
| Q3 | pass | `grep_filters_by_glob_and_ignores_case` |
| Q4 | pass | `grep_caps_at_200_matches` |
| Q5 | pass | `grep_cuts_a_long_line` |
| Q6 | pass | `grep_rejects_a_bad_regex` |
| Q7 | pass | `search_tools_stay_in_the_worktree` |
| Q8 | pass | `walks_do_not_follow_a_symlink_out` |
| Q9 | pass | `glob_finds_unignored_files_sorted` |
| Q10 | pass | `glob_reports_no_files_and_bad_patterns` |
| Q11 | pass | `code_map_outlines_rust` |
| Q12 | pass | `code_map_covers_each_language` |
| Q13 | pass | `code_map_caps_its_output` |
| Q14 | pass | `code_map_survives_a_parse_error` |
| Q15 | pass | `binary_files_are_skipped` |
| Q16 | pass | `both_formats_get_the_new_tools` |
| Q17 | waiting on the human | This shell has no provider key. |

Yield: 17 cases. 16 ran and passed, 0 failed. Q17 waits on a live run.
