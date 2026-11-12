# QA plan: Ram Code finds and installs its updates

The automated cases go in a new file, `tests/qa_update.rs`. Each test builds its own install
folder in a temp folder, and points the cache and the config at that temp folder.

- A release test serves the releases from a local HTTP server on a free port. `/latest` answers
  302 with a `Location` that ends in `/tag/<tag>`.
- The update tests point `RAMIFY_RELEASES` at a `file://` folder that holds the archive and its
  `.sha256`, so that the real `install.sh` runs with no network.
- A checkout test makes a bare `origin` repo and a clone, and commits to `origin` to put the
  clone behind.
- The cargo build of a checkout update is replaced by `RAM_CODE_BUILD`, a command that the test
  sets to a script that logs its call. The real build runs only in Q16.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `install_kind_is_read_from_the_folder`: `RELEASE` with `v0.2.0` gives release `v0.2.0`; a git checkout gives checkout with its commit; an empty folder gives unknown, and the check makes no request | build |
| Q2 | AC2 | high | `a_newer_release_is_an_update`: the server answers `v0.3.0`, the install is `v0.2.0`; the check finds `v0.3.0` | build |
| Q3 | AC2 | medium | `the_same_release_is_no_update`: the server answers `v0.2.0`; the check finds none | build |
| Q4 | AC3 | high | `a_checkout_behind_main_counts_the_commits`: 4 commits on `origin`; the check finds 4 | build |
| Q5 | AC4 | medium | `a_fresh_cache_makes_no_request`: a cache 1 hour old; the server counts zero requests, and a checkout runs no `git fetch` (its `origin` is removed, and the check still answers from the cache) | build |
| Q6 | AC4 | low | `an_old_cache_checks_again`: a cache 25 hours old; the server counts one request | build |
| Q7 | AC5 | high | `a_silent_server_ends_the_check_in_time`: the server accepts and never answers; the check ends in 11 s or less with no update and no error | build |
| Q8 | AC6 | medium | `the_header_shows_the_update`: an `App` given an update draws `update v0.3.0 → ram update`; with none, no line starts with `update` | build |
| Q9 | AC7 | high | `ram_update_installs_a_newer_release`: `ram-code update` on a `v0.2.0` folder, a `v0.3.0` archive with a good checksum; the folder's `RELEASE` is `v0.3.0` | build |
| Q10 | AC8 | high | `a_bad_checksum_keeps_the_old_release`: a wrong `.sha256`; exit 1, `RELEASE` is still `v0.2.0` | build |
| Q11 | AC9 | high | `ram_update_fast_forwards_a_checkout`: a clean `main` 2 behind; after `ram-code update`, `main` equals `origin/main` and the build command ran for `cli/` and the root | build |
| Q12 | AC10 | high | `ram_update_refuses_a_checkout_it_must_not_move`: on another branch; with an uncommitted change; with a local commit not on `origin`. Each exits 1, names the reason, and `HEAD` is unchanged | build |
| Q13 | AC11 | medium | `ram_update_with_no_update_says_so`: exit 0 and `ram is up to date` | build |
| Q14 | AC12 | high | `auto_update_starts_at_quit`: `auto_update: true` and an update found; when the `App` quits, a detached `ram-code update` (a fake binary through `RAM_CODE_UPDATER`) logs its start to `update.log`. With the setting unset, nothing starts | build |
| Q15 | AC13 | medium | `no_update_turns_everything_off`: with `RAM_CODE_NO_UPDATE=1`, the server counts zero requests, the header has no update line, and auto-update starts nothing | build |
| Q16 | AC9 | medium | Live: in `~/dev/ramify-wt/main`, reset `main` 1 commit back, run `bin/ram update`; `main` is back on `origin/main`, the build runs, and `bin/ram --version` names the new commit | qa |
| Q17 | AC6 | low | Live: start `ram` in a checkout that is behind; the header shows the update line within a few seconds, and the interface opens at once | qa |
| Q18 | AC1 | low | `version_names_the_install`: `ram-code --version` prints the crate version, then the release tag or the short commit | build |
| Q19 | AC14 | low | The README documents the check, the header line, `ram update`, `auto_update` and `RAM_CODE_NO_UPDATE` | audit |
| Q20 | AC5 | medium | Read the diff: the check runs on its own thread, and the interface thread only reads its result | audit |

Cases added at the review:

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q21 | AC9 | high | `a_failed_build_is_built_again` | build |
| Q22 | AC3 | medium | `a_checkout_off_main_is_offered_nothing` | build |
| Q23 | AC5 | medium | `a_hung_fetch_ends_in_time` | build |
| Q24 | AC4 | medium | `the_cache_keeps_one_entry_per_install` | build |
| Q25 | AC6 | low | `a_tag_with_odd_characters_is_refused` | build |
| Q26 | AC7 | medium | `two_updates_at_once_refuse` | build |

## Write set

```
src/**
tests/qa_update.rs
README.md
Cargo.toml
Cargo.lock
.ramify/work/ram-update/**
!cli/**
!hooks/**
!install.sh
!workflows.yml
```
