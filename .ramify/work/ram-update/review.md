# Review: Ram Code finds and installs its updates

A fresh subagent reviewed `git diff $(git merge-base HEAD origin/main)..HEAD` against the spec
and the QA record. It ran the tests and clippy, and probed a checkout update with a failing
build. It reported 13 findings. The fixes are below. The spec has a review amendment (9–16).

| # | Severity | Where | Failure | Fix | Status |
|---|---|---|---|---|---|
| 1 | medium-high | `update_checkout` | A failed build after the fast-forward left mixed binaries. The next `ram update` said "up to date", and the header showed nothing. | `target/release/.ram-built` holds the commit of the last good build. It says `building` during a build. The check finds `Rebuild` when it is not HEAD, and `ram update` builds again. Test `a_failed_build_is_built_again`. | fixed |
| 2 | medium | `fetch`, `start_detached` | `ssh` could ask for a passphrase or a host key on the interface's terminal, and a timeout left its children. | `git fetch` and the detached update run in a new session (`setsid`). `GIT_SSH_COMMAND="ssh -o BatchMode=yes"` when the user has none. A timeout kills the group. Test `a_hung_fetch_ends_in_time`. | fixed |
| 3 | medium | `update_release` | The installer took `latest` or a `RAMIFY_VERSION` from the environment, linked into `~/.local/bin` whatever the user chose, and a failure after the swap said "the old release stays". | `--version <tag found>`, `RAMIFY_VERSION` removed, `--bin-dir` of the current `ram` link on PATH. A failure reads `RELEASE` and says what is installed. `ram_update_installs_a_newer_release` sets `RAMIFY_VERSION=v0.1.0`, serves the archive only under `download/v0.3.0`, and checks the link. | fixed |
| 4 | medium | `update_release` | The installer copy had a predictable name in a shared `/tmp`. | No copy: bash runs `<home>/install.sh` in place and keeps the file open through the swap. | fixed |
| 5 | medium | `look` | A checkout on a feature branch got an update line every day for a command that refuses. | A checkout off `main` is offered nothing. Test `a_checkout_off_main_is_offered_nothing`. | fixed |
| 6 | medium | `update_checkout` | `bin/ram` runs a debug build that is newer than the release build, so an update could change nothing. | After the build, the release `ram-code` gets the current time. | fixed |
| 7 | low | `home` | With a shared `CARGO_TARGET_DIR`, the folder above the binary could be `~`, and `ram update` could move a dotfiles repo. | `home()` needs `bin/ram` and a `Cargo.toml` named `ram-code`. | fixed |
| 8 | low | `run` | Two updates at once could mix their folders. | `update.lock` with `flock`. Test `two_updates_at_once_refuse`. | fixed |
| 9 | low | `check` | The cache write was not atomic, and one entry served all installs. | One entry per install folder, written with a rename. `ram update` clears the cache. Test `the_cache_keeps_one_entry_per_install`, which also changes the tag and checks again. | fixed |
| 10 | low | `newer` | Pre-releases compared equal, and `v1.2` was newer than `v1.2.0` for ever. | Semantic versions: missing parts are 0, and a pre-release comes before its release. Unit test `versions_compare_as_semver`. | fixed |
| 11 | low | `build`, `start_detached` | `RAM_CODE_BUILD` and `RAM_CODE_UPDATER` worked in release builds. | Only in a debug build. | fixed |
| 12 | low | `latest_tag` | A server could put control characters in the header through the tag. | Only `[A-Za-z0-9._+-]`, 64 characters at most. Test `a_tag_with_odd_characters_is_refused`. | fixed |
| 13 | low | various | The installer had no time limit, `update.log` grew for ever, the user got no message when an update started at quit, and the tests needed `shasum`. | 10 minutes at most for the installer. The log starts again past 1 MB. Ram Code prints the log path at quit. The tests fall back to `sha256sum`. | fixed |

Also found during the fixes: the tests ran `ram-code update` with the user's own cache folder.
`ram update` now takes a lock there and clears the check, so every test run of the binary gets
a temp cache.

Checks after the fixes: `cargo test` passes (`tests/qa_update.rs` 22, and the unit test), and
`cargo clippy --all-targets` reports nothing.
