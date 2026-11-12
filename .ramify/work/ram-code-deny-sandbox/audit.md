# Audit: Ram Code deny-by-default sandbox

| AC | Verdict | Code | Test |
|---|---|---|---|
| AC1 | met | `src/sandbox.rs` `seatbelt_profile_for` starts with `(deny default)` | `profile_starts_with_deny_default`, `unlisted_operation_fails` |
| AC2 | met | `SYSTEM_READS`, `HOME_READS`, `HOME_FILES`, `SERVICES`, `ar`'s temp name in the per-user temp folder, a per-task clang module cache | `toolchains_work_in_the_sandbox`, `c_build_script_works_in_the_sandbox`, `git_works_in_the_sandbox`, `common_tools_work_in_the_sandbox` |
| AC3 | met | read roots only | `reads_outside_the_roots_fail` |
| AC4 | met | the #62 denials come last | `tests/qa_sandbox.rs` passes. After the review, one fixture file moved (see below) |
| AC5 | met | `(allow signal (target same-sandbox))` | `cannot_signal_outside` |
| AC6 | met | a linked worktree's common Git folder is readable, and `.git` writes stay denied | `linked_worktree_git_reads_common_dir` |
| AC7 | met | `sandbox.json` in `RAM_CODE_CONFIG_DIR` only; secrets and `.git` stay denied | `sandbox_json_adds_paths`, `sandbox_json_cannot_unhide_secrets`, `project_sandbox_json_is_ignored` |
| AC8 | met | validation before a command runs | `invalid_sandbox_json_is_an_error` |
| AC9 | met | `DENIED_HINT` | `denied_command_gets_the_hint` |
| AC10 | met | README sandbox section | Q17 below |

## Q16, read the profile

Every `allow` is narrow, with one exception, which is now fixed (`66e148d`).

**The Keychain daemon was reachable.** `SERVICES` allowed `com.apple.SecurityServer`. An item stored with the `security` command often lets that command read it without a prompt. That covers ramify's Jira token (`cli/src/jira.rs`) and the MCP OAuth tokens to come. A sandboxed `security find-generic-password -w` could therefore put a secret in the transcript.

- **Evidence:** the new test `keychain_is_out_of_reach` creates a throwaway keychain in the test worktree, unlocked, with one dummy item. It never touches the user's keychains or search list. Before the fix, the sandboxed command read the dummy item (`exit 0`). After it, the read fails.
- **Fix:** the profile denies the Keychain services after the allows.
- **On `main` today:** #62's allow-by-default profile allows this lookup too, so `main` has the same hole until this branch merges.

**Other items checked:**
- `process-exec` is allowed, but `open`, `osascript`, `osacompile` and `launchctl` stay denied, as do Apple events and Launch Services.
- The #62 denials come after the allows.
- Each read root has a reason in a comment.
- `/Library` is the system folder, not `~/Library`.

**Deviations the build made, accepted:**
- Only files directly in the per-user temp folder are writable, not its subfolders. The review later narrowed this to `ar.XXXXXX` only (`4d20889`).
- `file-read-metadata` stays allowed everywhere, because the asdf `node` shim needs it. A command can learn that a path exists, and the README says so.
- Writes to the secret paths and to `~/.config/ram-code` are denied, so a command cannot widen its own sandbox through `sandbox.json`.

## Q17, read the README

The README describes deny-by-default, the read roots, `sandbox.json`, the per-user temp and cache folders, the hint, and now the Keychain.

## Checks

`cargo test`: 240 passed, 0 failed. `cargo clippy --all-targets`: no warnings. `ramify write-set .ramify/work/ram-code-deny-sandbox/write-set.txt`: the diff stays inside the write set. By hand, under the profile printed from the code: `cargo test --lib`, `git status`/`log`/`diff`, `make`, `python3`, `node`, `npm`, `grep`, `sed`, `awk`, `sh`, `bash` and `zsh` work.

## After the review

Review finding 4 removed reads of the per-user temp folder. Two tests had put files there and passed only through that read:

- In `tests/qa_sandbox.rs` `sandbox_hides_secret_files`, the ordinary file the sandbox must read moved from the fake home to the worktree. Under deny-by-default, a home file outside the read roots is unreadable anyway. Every assertion stays.
- In `tests/qa_hooks.rs` `post_workflow_gets_details_and_output`, the fake `ramify` moved from the fixture folder into the test worktree.

