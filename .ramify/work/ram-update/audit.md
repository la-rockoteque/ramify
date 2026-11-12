# Audit: Ram Code finds and installs its updates

The diff is `git diff $(git merge-base HEAD origin/main)`: `src/update.rs` (new), `src/app.rs`,
`src/main.rs`, `src/lib.rs`, `tests/qa_update.rs` (new) and `README.md`.

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `update::install_of` reads `RELEASE`, then `.git`. `check` returns at once for `Install::Unknown`. Test `install_kind_is_read_from_the_folder` (and the server counts zero requests). |
| AC2 | met | `update::look` with `latest_tag` and `newer`. Tests `a_newer_release_is_an_update`, `the_same_release_is_no_update`, and the unit test `a_higher_number_is_newer`. |
| AC3 | met | `update::look` runs `fetch`, then `count(home, "main..origin/main")`. Test `a_checkout_behind_main_counts_the_commits`. |
| AC4 | met | `update::check` reuses `update.json` when its `key` matches and it is younger than `CHECK_EVERY`. Tests `a_fresh_cache_makes_no_request` (the checkout half removes `origin`) and `an_old_cache_checks_again`. |
| AC5 | met | `latest_tag` sets `timeout_global(CHECK_TIMEOUT)`; `fetch` kills `git` at the same deadline. A failed check returns `None` and is not cached. Test `a_silent_server_ends_the_check_in_time`. |
| AC6 | met | `App::render_header` adds `Found::line` in yellow, and `render_chat` gives the header one more row. Test `the_header_shows_the_update`. |
| AC7 | met | `update::update_release` runs a copy of the folder's `install.sh --home <folder>`. Test `ram_update_installs_a_newer_release` runs the real installer against the test server. |
| AC8 | met | The installer refuses a wrong checksum before the swap, and `ram update` exits 1. Test `a_bad_checksum_keeps_the_old_release`. |
| AC9 | met | `update::update_checkout` merges `--ff-only`, then `build` for `cli/Cargo.toml` and `Cargo.toml`. Test `ram_update_fast_forwards_a_checkout`, with `RAM_CODE_BUILD` in place of cargo. The real build is Q16. |
| AC10 | met | `update_checkout` checks the branch, `git status --porcelain --untracked-files=no` and the commits ahead, before any merge. Test `ram_update_refuses_a_checkout_it_must_not_move` covers the three refusals and checks that no build ran. |
| AC11 | met | `update::run` prints `ram is up to date (<version>)`. Test `ram_update_with_no_update_says_so`. |
| AC12 | met | `App::auto_update` needs an update found, `auto_update` on and no `RAM_CODE_NO_UPDATE`. `update::start_detached` starts its own process group with `update.log` as output. `main` calls it after the interface ends. Test `auto_update_starts_at_quit`. |
| AC13 | met | `update::disabled` stops `start_update_check` and `auto_update`. Test `no_update_turns_everything_off`, which then unsets the variable to show that the same app checks. |
| AC14 | met | README section "Updates". |

## QA cases settled by the audit

| Case | Verdict | Evidence |
|---|---|---|
| Q19 | pass | README "Updates": the check, the header line, `ram update`, `auto_update`, `RAM_CODE_NO_UPDATE` and `ram --version`. |
| Q20 | pass | `App::start_update_check` runs `update::check` on a spawned thread and writes `update_slot`. The interface thread only takes the slot in `poll_events` and `auto_update`. |

## Deviations from the plan

- Q9 and Q10 serve the archive from the local HTTP server, not from a `file://` folder. The
  check of `ram update` reads `/latest` first, and that needs a redirect.

## Write set

`ramify write-set qa-plan.md`: the diff stays inside the write set.
