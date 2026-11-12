# Ram Code deny-by-default sandbox

## Problem

Ram Code's macOS sandbox from #62 starts from `(allow default)` and adds denials. Each new escape needs a new denial, and the review of #62 found four. Anything not denied stays open. A command can read `~/Documents`, the user's other repositories, `~/.zshrc` or the system logs, and the output goes to the provider in the transcript. It can also run `osascript` only because a denial blocks it by name. Codex starts from `(deny default)` and allows what tools need. This branch moves Ram Code's macOS profile to the same model.

## Behaviour

### The profile

On macOS, `seatbelt_profile` starts with `(deny default)`. It then allows only the following. Everything else is denied.

- **Processes**: `process-exec` and `process-fork`. Signals and `process-info*` only to processes in the same sandbox.
- **System reads**: `sysctl-read` and `user-preference-read`. `file-read-metadata` everywhere, so that `stat` and path lookups work.
- **IPC**: POSIX semaphores and shared memory.
- **Services**: `mach-lookup` for a short named list. The list covers directory lookups (`com.apple.system.opendirectoryd.libinfo`, `com.apple.system.DirectoryService.libinfo_v1`), logging (`com.apple.system.logger`, `com.apple.logd`, `com.apple.diagnosticd`), notifications (`com.apple.system.notification_center`), certificates (`com.apple.trustd.agent`, `com.apple.SecurityServer`) and `com.apple.bsd.dirhelper`. Launch Services is not on the list.
- **File reads** under these roots only:
  - System: `/usr`, `/bin`, `/sbin`, `/System`, `/Library`, `/private/etc`, `/private/var/db/timezone`, `/private/var/select`, `/dev`, and `/opt/homebrew` when it exists. Also `/Applications/Xcode.app` and `/Library/Developer`, for the compilers.
  - The worktree, the task's temp folder, and the repository's common Git folder (`git rev-parse --git-common-dir`). A linked worktree keeps its Git data there.
  - The user's temp and cache folders (`getconf DARWIN_USER_TEMP_DIR` and `DARWIN_USER_CACHE_DIR`). `xcrun` and `clang` write there and ignore `TMPDIR`.
  - Toolchains in the home folder, when they exist: `~/.cargo`, `~/.rustup`, `~/.asdf`, `~/.nvm`, `~/.pyenv`, `~/.volta`, `~/.local/share/mise`, `~/.npm`, `~/.tool-versions`.
  - Git settings: `~/.gitconfig` and the files whose name starts with it (`include.path` targets such as `~/.gitconfig-github`), and `~/.config/git`.
- **File writes** under the worktree, the task's temp folder, the user's temp and cache folders, `/dev/null`, `/dev/zero`, `/dev/dtracehelper` and `/dev/fd/*`.

Then come the denials from #62, last, so they win over every allowance:

- reads of the secret paths;
- writes to any `.git` path in the worktree (the common Git folder is readable, never writable);
- `process-exec` of `open`, `osascript`, `osacompile` and `launchctl`;
- `file-ioctl` on `/dev/tty*`.

`network*` is not allowed at all, so Unix sockets stay closed.

### Extending the profile

The user can add paths in `~/.config/ram-code/sandbox.json` (`RAM_CODE_CONFIG_DIR` moves it):

```json
{ "read": ["~/work/shared-fixtures"], "write": ["~/.cache/my-tool"] }
```

- Paths are absolute or start with `~/`. Ram Code resolves each to its real path.
- A listed path under a secret path, or a `.git` path, stays denied. The denials come last.
- A project cannot extend its own sandbox: Ram Code reads no sandbox file from the worktree.

### When the sandbox denies something

When a sandboxed command's output contains `Operation not permitted`, the tool result ends with one hint line. The hint says the sandbox may have denied the command. It names `sandbox.json` for a lasting need and `outside_sandbox` for a one-time need.

### Linux

`bwrap` stays as #62 left it. A deny-by-default filesystem view needs a bind for each read root. That is a later branch.

### Measured on this machine

A draft of this profile ran on macOS 27.2, from a fresh worktree of this repository, with toolchains from asdf:

- These worked: `cargo build`; `cargo test --lib`, with a C build script (`ring`); `git status`, `git log` and `git diff`; `make`, `python3`, `node`, `npm --version`, `grep`, `sed` and `awk`; `sh`, `bash` and `zsh`.
- These failed as intended: reads of `~/Documents`, `~/dev`, `~/.zshrc`, `/Users` and `/private/var/log/system.log`; `kill -0 1`; a TCP connection to `127.0.0.1`; a write in the home folder; and `touch /dev/tty`.
- Two findings shaped the rules above:
  - Without the user's temp and cache folders, `clang` failed with `unable to make temporary file`.
  - Without the explicit denial, `osascript` still ran, because `process-exec` is allowed.
- `ps -A` printed nothing, because `process-info` covers only the sandbox's own processes.

States:

- Success: a command that needs only the allowed paths runs as before.
- Denied: the command fails with `Operation not permitted`, and the tool result adds the hint.
- Error: an unreadable `sandbox.json` makes the tool return an error that names the file. Nothing runs unconfined.

## Acceptance criteria

AC1. Given the macOS profile, then it starts with `(deny default)`, and an operation not on the allow list fails. For example, `ls /Users` fails with `Operation not permitted`.

AC2. Given a fresh worktree of this repository with an empty `target/`, then these succeed in the sandbox: `cargo build`, `cargo test --lib`, `git status`, `git log -1`, `git diff --stat HEAD~1`, `make --version`, `python3 -c 'print(1)'`, `node -e 'console.log(1)'`, `npm --version`, `grep`, `sed` and `awk` on a worktree file, and `sh`, `bash` and `zsh` running `echo`. Each tool runs only when it is installed.

AC3. Given files outside the read roots, then reading them fails: a file in a temp folder outside the allowed ones, a sibling repository, `~/.zshrc`, and a listing of `/Users`.

AC4. Every #62 guarantee holds. The `tests/qa_sandbox.rs` cases for secret reads, `.git` writes, network and Unix sockets, the terminal, and launches pass unchanged.

AC5. Given a process outside the sandbox, then a sandboxed `kill -0 <pid>` fails.

AC6. Given a linked worktree, then `git status` reads the common Git folder in the sandbox. A write to a file in that folder fails.

AC7. Given `sandbox.json` with a `read` path and a `write` path, then a sandboxed command can read the first and write the second. A listed `~/.ssh` path stays unreadable. A `sandbox.json` inside the worktree has no effect.

AC8. Given a `sandbox.json` that is not valid JSON, then `run_command` returns an error that names the file, and the command does not run.

AC9. Given a sandboxed command that fails with `Operation not permitted`, then the tool result ends with the hint line, which names `sandbox.json` and `outside_sandbox`.

AC10. The README's sandbox section describes the deny-by-default profile, the read roots, `sandbox.json` and the hint.

## Out of scope

- Linux: a deny-by-default `bwrap` view.
- A project-level sandbox file.
- Network allow lists.
- Toolchain managers beyond the list above. A user adds them in `sandbox.json`.

## Decisions

The user settled the open questions on 2026-11-12.

1. Reads are allowed only under the listed roots. `~/Documents`, other repositories and dotfiles stay unreadable. `~/.config/ram-code/sandbox.json` adds paths.
2. Writes are allowed to the per-user temp folder and to clang's cache, not to the rest of `~/Library/Caches`, so C builds work in the sandbox. The README says that a command could plant a file there that another program reads later.

## Lane

Standard, tag `permissions`. The branch changes what the model may do on the user's machine.
