# Review: Ram Code deny-by-default sandbox

A separate, security-minded reviewer agent printed the profile from the code and ran each attack under `sandbox-exec`. It edited nothing in the worktree. Every finding is fixed, with a test that failed on the old code. The fixes are in `d967748` to `cb3a65c`.

| # | Severity | Failure | Fix | Test |
|---|---|---|---|---|
| 1 | critical | A command could replace its task temp folder with a symlink. The next command's profile followed the link and allowed writes to its target, such as `~/Library/LaunchAgents`. Demonstrated. `main` (#62) has the same hole. | The temp path is fixed when the task starts. Before each command, Ram Code checks that it is still a real folder the user owns, and refuses with "was replaced" if not. The profile denies unlinking or renaming the temp folder and the worktree. | `temp_folder_cannot_be_repointed` |
| 2 | high | `~/.config/git/credentials`, a registry token in `~/.cargo/config.toml`, and tokens in `~/.gitconfig` URLs were readable. Demonstrated with a fake home folder. | `~/.config/git/credentials` is a secret. `~/.config/git` reads are narrowed to `config`, `ignore` and `attributes`, and `~/.cargo` to `bin`, `registry`, `git`, `env` and `config.toml`. | `git_and_cargo_credentials_stay_hidden` |
| 3 | high | With `~/.config` a symlink, as dotfile managers make it, a command read `gh`'s token and rewrote `sandbox.json` to widen every later command. Demonstrated. | Secrets and the config folder are denied by the given path and the real path. | `symlinked_secret_and_config_paths_stay_denied` |
| 4 | medium | The per-user temp folder was readable, and its entries could be renamed or swapped for links. Demonstrated. | Only `ar.XXXXXX` files can be created, read or written there, because `xcrun` points `ar` at that folder. `xcrun_db` is read-only. | `per_user_temp_is_create_only` |
| 5 | low | The shared clang module cache was writable, so a planted module could change a later build outside the sandbox. | `CLANG_MODULE_CACHE_PATH` is per task, and the shared cache is read-only. | `shared_clang_cache_is_not_writable` |
| — | low | `xcrun`'s cache warning added the sandbox hint to successful builds. | No hint for that warning. | `xcrun_cache_warning_gets_no_hint` |

The reviewer found these sound:
- Services outside the allow-list are denied: the clipboard read 0 bytes, other apps' preferences failed, and a copied `launchctl` is killed.
- The profile cannot be injected.
- Rule order holds: denials win over the read roots and over `sandbox.json`.

## Kept with a reason

`~/.cargo/config.toml` stays readable: Cargo stops when the file exists and cannot be read. The README tells users to keep registry tokens in `credentials.toml`, which is a secret.

## Residual risks, in the README

- `file-read-metadata` shows that a file exists, and its size.
- Tokens in `~/.gitconfig` `insteadOf` URLs and in `~/.cargo/config.toml` stay readable.
- `com.apple.trustd.agent` stays allowed for certificate checks. The reviewer saw no outbound fetch through it.

## Changes to the record

- Two test fixtures in `tests/qa_sandbox.rs` and `tests/qa_hooks.rs` read the per-user temp folder, which the sandbox now denies. They moved, so the plan's claim that `qa_sandbox.rs` passes unchanged no longer holds.
- **A real file was touched during the fix.** While checking the per-user temp test against the old rule, the fix work appended to the real `xcrun_db` in the per-user temp folder. `xcrun` detected it and rebuilt its cache. `xcrun --find clang` works afterwards. The check now appends zero bytes, so it cannot change a byte.

## Checks after the fixes and the merge with `main` (#63)

- `cargo test --no-fail-fast`: 272 passed, 0 failed. The `cli` tests pass.
- `cargo clippy --all-targets`: no warnings.
- `ramify write-set .ramify/work/ram-code-deny-sandbox/write-set.txt`: the diff stays inside the write set.
- By hand, through `Tools` with the real home folder: `cargo test --lib`, `git status`/`log`/`diff`, `make`, `python3`, `node`, `npm`, `sh` and `bash` work. `zsh` works in `common_tools_work_in_the_sandbox`. `cat ~/.zshrc` and `ls ~/Documents` are denied with the hint.
