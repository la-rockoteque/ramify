# Audit: Ram Code search tools

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `src/search.rs` `files` (gitignore-aware walk, `.git` filtered), `grep` | `grep_skips_ignored_files`, `grep_skips_the_git_folder` |
| AC2 | met | `src/search.rs` `grep`: `glob` filter on the path or file name, `RegexBuilder::case_insensitive` | `grep_filters_by_glob_and_ignores_case` |
| AC3 | met | `src/search.rs` `MAX_MATCHES`, `MAX_LINE` | `grep_caps_at_200_matches`, `grep_cuts_a_long_line` |
| AC4 | met | `src/tools.rs` `Tools::resolve` for the requested path. `src/search.rs` `files` walks without following links and drops any path whose real path leaves the worktree. | `grep_rejects_a_bad_regex`, `search_tools_stay_in_the_worktree`, `walks_do_not_follow_a_symlink_out` |
| AC5 | met | `src/search.rs` `glob` | `glob_finds_unignored_files_sorted`, `glob_reports_no_files_and_bad_patterns` |
| AC6 | met | `src/search.rs` `code_map`, `outline` with tree-sitter-rust | `code_map_outlines_rust` |
| AC7 | met | `src/search.rs` `language_of`, `outline`, `MAX_MAP` | `code_map_covers_each_language`, `code_map_caps_its_output`, `code_map_survives_a_parse_error` |
| AC8 | met | `src/search.rs` `is_binary` (a NUL byte in the first 8 KiB) | `binary_files_are_skipped` |
| AC9 | met | `src/tools.rs` `Tools::definitions` feeds both `AnthropicLlm` and `openai::to_chat` | `both_formats_get_the_new_tools` |

## Notes

- The build tightened Q7: its first version passed through an "unknown tool" error. It now requires the worktree path error.
- Every walk starts at the worktree root, so a `.gitignore` above the requested folder still applies. The cost is a full-tree walk on each call. That is fine for a repo this size. A large monorepo may need a walk from the requested folder later.
- Symlinks are skipped, even those that stay inside the worktree. That is stricter than the spec asks and closes the escape route.
- A missing folder gives `no files` or `no matches`. The spec did not cover that case.

## Checks

`cargo test` passes in every suite. `cargo clippy --all-targets` reports no warnings. `ramify write-set .ramify/work/ram-code-search-tools/write-set.txt`: the diff stays inside the write set.
