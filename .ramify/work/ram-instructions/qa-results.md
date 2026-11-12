# QA results: Ram Code reads AGENTS.md and CLAUDE.md

The build cases ran with `cargo test --test qa_instructions`: 16 passed. The full suites of
both crates and `cargo clippy --all-targets` pass.

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `user_files_come_first` |
| Q2 | pass | `project_files_come_in_order` |
| Q3 | pass | `a_parent_folder_comes_first` |
| Q4 | pass | `a_linked_file_is_read_once` |
| Q5 | pass | `an_import_brings_the_file_in` |
| Q6 | pass | `a_home_import_reads_from_home` |
| Q7 | pass | `an_import_loop_ends` |
| Q8 | pass | `imports_stop_at_depth_five` |
| Q9 | pass | `a_missing_import_stays_as_text` |
| Q10 | pass | `a_large_file_is_cut` |
| Q11 | pass | `all_files_stop_at_128_kib` |
| Q12 | pass | `a_file_that_is_not_utf8_is_skipped` |
| Q13 | pass | `no_file_keeps_the_prompt` |
| Q14 | pass | `an_openai_provider_gets_the_instructions` |
| Q15 | pass | `the_instructions_command_lists_the_files` |
| Q16 | pass | `the_ramify_repo_reads_its_agents_md_once` |
| Q17 | pass | audit: the paths and commands of `AGENTS.md` |
| Q18 | pass | audit: README section |
| Q19 | waived | No model provider is set up on this machine. The user waived the live task. Q1–Q16 check the system prompt that each task sends. |

Cases added at the review:

| Case | Result | Evidence |
|---|---|---|
| Q20 | pass | `a_project_cannot_import_outside_itself` |
| Q21 | pass | `a_linked_claude_md_outside_the_project_is_skipped` |
| Q22 | pass | `a_repo_under_home_is_named_from_its_top` |
| Q23 | pass | `an_import_is_cut_on_its_own` |
| Q24 | pass | `the_listing_shows_what_the_limits_did` |
| Q25 | pass | `no_home_reads_only_the_project` |
| Q26 | pass | `a_cut_keeps_whole_characters` |
| Q27 | pass | `agents_md_has_no_imports_and_an_email_is_no_import` |
| Q28 | pass | `a_tilde_fence_closes_only_on_a_tilde_fence` |
| Q29 | pass | `a_linked_worktree_reads_its_own_files` |

**Yield:** 29 cases. 28 ran, 28 passed, 0 failed. The user waived Q19.
