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

**Yield:** 19 cases. 18 ran, 18 passed, 0 failed. The user waived Q19.
