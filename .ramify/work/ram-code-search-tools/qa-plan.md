# QA plan: Ram Code search tools

The repo declares no services. The build settles the automated cases with `cargo test`. The tests build small worktrees in temporary folders, each with a `.gitignore`, a symlink out, and a binary file where a case needs one.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `grep_skips_ignored_files`: a match in a tracked file and in a `.gitignore`d file. Only the tracked match comes back, as `path:line: text`. | build |
| Q2 | AC1 | medium | `grep_skips_the_git_folder`: a match inside `.git/` is not returned. | build |
| Q3 | AC2 | medium | `grep_filters_by_glob_and_ignores_case`: `glob: "*.rs"` with `ignore_case: true` finds `Foo` in `a.rs` from the pattern `foo`, and skips `b.txt`. | build |
| Q4 | AC3 | medium | `grep_caps_at_200_matches`: 250 matching lines give 200 lines and a note that the output was cut. | build |
| Q5 | AC3 | low | `grep_cuts_a_long_line`: a 1,000-character matched line comes back at 300 characters plus a marker. | build |
| Q6 | AC4 | high | `grep_rejects_a_bad_regex`: the pattern `(` gives an error that quotes the parser's message. | build |
| Q7 | AC4 | high | `search_tools_stay_in_the_worktree`: `../`, an absolute path, and a path through a symlink that leaves the worktree give an error for `grep`, `glob` and `code_map`. | build |
| Q8 | AC4 | high | `walks_do_not_follow_a_symlink_out`: a symlinked folder that points outside, searched from `.`, adds no result from outside. | build |
| Q9 | AC5 | high | `glob_finds_unignored_files_sorted`: `src/**/*.rs` returns every `.rs` file under `src`, sorted, without the ignored one. | build |
| Q10 | AC5 | low | `glob_reports_no_files_and_bad_patterns`: no match gives `no files`; the pattern `[` gives an error. | build |
| Q11 | AC6 | high | `code_map_outlines_rust`: a struct, an impl with two methods, and a free function. Each has its kind, name and line, and the methods are indented under the impl. | build |
| Q12 | AC7 | high | `code_map_covers_each_language`: TypeScript, JavaScript, Python and Java files each give at least their class and function with lines. A Markdown file shows its line count only. | build |
| Q13 | AC7 | medium | `code_map_caps_its_output`: a folder whose outline passes 40,000 characters is cut at a file boundary, with a note. | build |
| Q14 | AC7 | low | `code_map_survives_a_parse_error`: a Rust file with a syntax error still gives the symbols tree-sitter found and does not fail the call. | build |
| Q15 | AC8 | medium | `binary_files_are_skipped`: a file with NUL bytes that contains the pattern is not matched by `grep` or listed by `code_map`. | build |
| Q16 | AC9 | medium | `both_formats_get_the_new_tools`: the Anthropic request's `tools` and the Chat Completions `tools` both name `grep`, `glob` and `code_map`. | build |
| Q17 | AC1–AC7 | medium | In Ram Code on this repo, ask "where is `strip_thinking` defined and what calls it?". The transcript shows `grep` or `code_map` calls before any `read_file`, and the answer names `src/agent.rs`. | qa |

## Write set

May change: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/**`, `README.md`, `.ramify/work/ram-code-search-tools/**`.

Must not change: `bin/**`, `cli/**`, `app/**`, `skills/**`, `hooks/**`, `.claude-plugin/**`, `.codex-plugin/**`, other `.ramify/work/**` folders.
