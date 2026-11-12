# Spec: Ram Code finds and installs its updates

## Problem

Ram Code never tells its user that a newer version exists. A user who installed a release must
remember to run the installer again. A user who runs a checkout must pull and build by hand, and
`ram` keeps running old code until then. Ram Code must find each update and install it with
one command, or by itself when the user asks for that.

## Behaviour

1. Ram Code knows how it is installed. Its install folder is the folder above
   `target/release/ram-code`.
   - **release**: the folder has a `RELEASE` file. The version is the tag in that file, such
     as `v0.3.0`.
   - **checkout**: the folder is a git checkout. The version is its commit.
   - **unknown**: anything else. Ram Code does not check and does not update.
2. When the interface starts, a background thread checks for an update. The interface never
   waits for it.
   - **release**: it reads the tag of the latest GitHub release. `RAMIFY_RELEASES` changes
     the releases URL, as it does for the installer.
   - **checkout**: it runs `git fetch origin main`, then counts the commits that `main` lacks
     from `origin/main`.
   - It checks at most once in 24 hours. It keeps the result in
     `~/.cache/ram-code/update.json`, and reuses that result until it is 24 hours old.
   - On a network error or a timeout of 10 s, it does nothing and shows nothing.
3. When an update exists, the header shows one line: `update v0.3.0 → ram update` for a release,
   or `update: 4 commits on main → ram update` for a checkout.
4. `ram update` (that is, `ram-code update`) installs the update, then prints what it did.
   - **release**: it runs the installer of the install folder (`install.sh --home <folder>`),
     copied to a temporary file first. The installer downloads the archive, checks its SHA-256
     and swaps the folder in one move. A failed update leaves the old version in place.
   - **checkout**: it fast-forwards `main` to `origin/main`, then builds `cli/` and Ram Code in
     release mode.
   - With no update, it prints `ram is up to date (<version>)` and exits 0.
   - On an error, it prints the reason and exits 1. Nothing is half installed.
5. A checkout updates only when all of these are true. Otherwise `ram update` refuses, and says
   which one is false.
   - The checkout is on `main`.
   - It has no uncommitted change.
   - `main` is behind `origin/main` and not ahead of it.
6. Auto-update is off by default. `"auto_update": true` in `~/.config/ram-code/settings.json`
   (or `RAM_CODE_CONFIG_DIR`) turns it on. Then, when the interface quits with an update
   found, Ram Code starts `ram-code update` as a detached process. It writes its output to
   `~/.cache/ram-code/update.log`. The next start uses the new version.
7. `RAM_CODE_NO_UPDATE=1` turns off the check, the header line and auto-update. `ram update`
   still works when the user runs it.
8. `ram --version` prints the version of 1: the release tag, or the short commit for a
   checkout, after the crate version.

## Acceptance criteria

- AC1: Given a folder with `RELEASE` containing `v0.2.0`, then the install is release `v0.2.0`.
  Given a git checkout, then the install is checkout at its commit. Given neither, then it is
  unknown and no check runs.
- AC2: Given a release install and a latest release `v0.3.0`, then the check finds the update
  `v0.3.0`. Given a latest release equal to the installed tag, then it finds none.
- AC3: Given a checkout on `main` that is 4 commits behind `origin/main`, then the check finds
  4 commits.
- AC4: Given a cached result younger than 24 hours, then the check makes no network request
  and runs no `git fetch`.
- AC5: Given a releases URL that does not answer, then the check ends in 10 s or less, finds
  nothing, and the interface shows no error.
- AC6: Given an update found, then the header shows the update line. Given none, then the
  header has no update line.
- AC7: Given a release install and a newer release with a good checksum, when `ram update`
  runs, then the folder holds the new release and `RELEASE` holds its tag.
- AC8: Given a release with a wrong checksum, when `ram update` runs, then it exits 1 and the
  folder still holds the old release.
- AC9: Given a checkout on `main` behind `origin/main` with no local change, when `ram update`
  runs, then `main` equals `origin/main` and both release binaries are built.
- AC10: Given a checkout on another branch, or with a local change, or ahead of `origin/main`,
  when `ram update` runs, then it exits 1, names the reason, and changes nothing.
- AC11: Given no update, when `ram update` runs, then it prints `ram is up to date` and
  exits 0.
- AC12: Given `auto_update: true` and an update found, when the interface quits, then a
  detached `ram-code update` starts and logs to `update.log`. Given `auto_update` unset, then
  nothing starts.
- AC13: Given `RAM_CODE_NO_UPDATE=1`, then no check runs, the header has no update line, and
  auto-update does not start.
- AC14: The README documents the check, the header line, `ram update`, `auto_update` and
  `RAM_CODE_NO_UPDATE`.

## Amendment: review (2026-11-12)

9. A checkout update writes the commit it built to `target/release/.ram-built`. A build that
   fails leaves a stamp that is not HEAD. Then the check finds `Rebuild`, and the next
   `ram update` builds again. After a build, the release binary is newer than any debug build.
10. A checkout on a branch other than `main` is offered nothing.
11. `git fetch` and the detached update run in their own session, with no terminal. `ssh` runs
    with `BatchMode=yes` unless the user set `GIT_SSH_COMMAND`. A timeout kills the whole group.
12. A release update runs the folder's `install.sh` in place, with `--version <tag found>`,
    without `RAMIFY_VERSION`, and with `--bin-dir` of the current `ram` link on PATH. A failure
    after the swap says that the new release is installed.
13. One `ram update` runs at a time (`update.lock`). The cache keeps one entry per install, and is
    written with a rename. `ram update` clears it.
14. A tag outside `[A-Za-z0-9._+-]`, or longer than 64 characters, is refused. Tags compare as
    semantic versions: `v1.2` equals `v1.2.0`, and `v0.3.0-rc.1` comes before `v0.3.0`.
15. `RAM_CODE_BUILD` and `RAM_CODE_UPDATER` work only in a debug build. `home()` accepts the
    folder above the binary only when it holds `bin/ram` and the `ram-code` `Cargo.toml`.
16. The installer may run 10 minutes at most. `update.log` starts again past 1 MB. At quit, Ram
    Code says that an update runs in the background, and names the log.

## Out of scope

- A check or an update for the `ramify` CLI alone. Both binaries come from one install, so one
  update covers both.
- A check in the headless mode (`--task`) and in the worker.
- A downgrade, or a choice of version. `install.sh --version` does that.
- A checkout whose main branch is not called `main`.
- Signature checks beyond the SHA-256 that the installer already does.

## Open questions

None. The user chose a notice and `ram update` first, with auto-update as an opt-in setting.

## Lane

Standard. Ram Code starts to use the network at start-up and to replace its own install.
