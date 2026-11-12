# Review: Ram Code search tools

A separate reviewer agent read the branch diff and the record. It ran the suites and probed edge cases with a scratch binary, and it edited nothing. All nine findings and both simplicity notes are fixed in `133eb54` and `92b814c`. Each fix has a test that failed first.

| # | Severity | Where | Failure | Fix | Test |
|---|---|---|---|---|---|
| 1 | medium | `src/search.rs` `visit` | A deeply nested expression overflowed the stack and aborted the whole session. Demonstrated at 20,000 levels. | The outline walks with an explicit stack. | `code_map_survives_deep_nesting` (runs in a child process on a 128 KiB stack; it failed with the old walk put back) |
| 2 | medium | `src/search.rs` `code_map` | One file over the 40,000 cap gave only the cut note. | The first file always shows, cut at a line boundary. | `code_map_cuts_one_big_file_at_a_line` |
| 3 | medium | `src/search.rs` `text_of` | Each file was read whole before the binary check; a 2 GB file cost about 4 GB. | 8 KiB binary check first. Files over 10 MB are skipped and named. Line counts read in chunks. | `large_files_are_skipped` |
| 4 | medium | `src/search.rs` `files` | A file `path` skipped the `.git` and `.gitignore` filters, against the spec. | Such a path is an error. | `a_file_path_under_git_or_ignored_is_rejected` |
| 5 | low | `src/search.rs` | `SRC` on a case-insensitive disk, `./src/*.rs`, and `glob` on a file all silently found nothing. | Real-path comparison, `./` dropped, file `path` in `glob` is an error. | `paths_and_patterns_normalise` |
| 6 | low | `src/search.rs` | `grep`'s `glob` filter and the `glob` tool matched differently. | One rule: no `/` matches names at any depth; with `/`, the path from `path`. | `grep_glob_matches_like_the_glob_tool` |
| 7 | low | `src/search.rs` `MAX_MAP` | The cap counted bytes while the note said characters, and the note passed the cap. | The cap counts characters and the note fits inside. | `code_map_cap_counts_what_it_says` |
| 8 | low | `src/search.rs` `files` | Every call walked the whole tree and canonicalized every file. | The walk enters only folders on the way to the requested path and under it, so the root `.gitignore` still applies. The per-file check is gone; `follow_links(false)` already drops symlinks. | `subfolder_walk_keeps_root_gitignore` |
| 9 | low | `src/search.rs` outline | TS class-field arrow functions and unnamed default exports were missing. | Listed as methods, and as `default`. | `ts_class_fields_and_default_exports_are_listed` |

Simplicity: one `PathFilter` holds the shared glob setup, and the `display` wrapper is gone.

## Behaviour change to note

`glob` with a pattern without `/`, such as `*.rs`, now matches at any depth. Before, it matched the top level of `path` only. The README and the tool description say so.

## Checks after the fixes

`cargo test` passes in every suite. One test is ignored on purpose: it is the child process that `code_map_survives_deep_nesting` runs. `cargo clippy --all-targets` reports no warnings. `ramify write-set .ramify/work/ram-code-search-tools/write-set.txt`: the diff stays inside the write set.
