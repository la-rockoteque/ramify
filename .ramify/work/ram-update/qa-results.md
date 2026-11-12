# QA results: Ram Code finds and installs its updates

The build cases ran with `cargo test --test qa_update`: 16 passed. The full suite and
`cargo clippy --all-targets` pass.

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `install_kind_is_read_from_the_folder` |
| Q2 | pass | `a_newer_release_is_an_update` |
| Q3 | pass | `the_same_release_is_no_update` |
| Q4 | pass | `a_checkout_behind_main_counts_the_commits` |
| Q5 | pass | `a_fresh_cache_makes_no_request` |
| Q6 | pass | `an_old_cache_checks_again` |
| Q7 | pass | `a_silent_server_ends_the_check_in_time` |
| Q8 | pass | `the_header_shows_the_update` |
| Q9 | pass | `ram_update_installs_a_newer_release` |
| Q10 | pass | `a_bad_checksum_keeps_the_old_release` |
| Q11 | pass | `ram_update_fast_forwards_a_checkout` |
| Q12 | pass | `ram_update_refuses_a_checkout_it_must_not_move` |
| Q13 | pass | `ram_update_with_no_update_says_so` |
| Q14 | pass | `auto_update_starts_at_quit` |
| Q15 | pass | `no_update_turns_everything_off` |
| Q16 | pass | Live. `~/dev/ramify-wt/main` was reset to `b16f19c`. The branch's `ram-code update` with `RAM_CODE_HOME` on that worktree printed `ram updated from main@b16f19c to main@dbd62ef`. `main` equals `origin/main`, and both release binaries have a new time (13:26). |
| Q17 | waived | The user waived the look at the live header. The branch's binary, started on a clone 1 commit behind, ran the check at start-up and wrote `{"found":{"commits":14}}` to the cache. Q8 checks the drawn header. |
| Q18 | pass | `version_names_the_install` |
| Q19 | pass | audit: README "Updates" |
| Q20 | pass | audit: the check runs on its own thread |

**Yield:** 20 cases. 19 ran, 19 passed, 0 failed. The user waived Q17.
