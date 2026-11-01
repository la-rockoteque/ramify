# ramify in Rust

## Problem

The ramify CLI is 1.7k lines of bash plus about 1.5k lines of Python across 65 files. On this
machine `ramify dash --json` takes 49 seconds: it runs one `dash-project` per project and each
one probes services with `curl`, `docker ps` and `lsof`. Parsing and state handling are
brittle, and every command pays the cost of starting a shell pipeline.

A single Rust binary gives the same behaviour in milliseconds, with types and tests.

## Behaviour

A Rust binary `ramify` implements every command the shell CLI has today. The package lives in
`cli/`, so the build is `cargo build --release --manifest-path cli/Cargo.toml`. During the
migration `bin/ramify` stays the entry point: it delegates a command to the Rust binary when
that command is ported, and falls back to the shell otherwise. At the end the shell
implementation is removed and `bin/ramify` execs the binary.

Parity means, for each command:

- the same exit codes for success, usage error and failure;
- byte-identical `--json` output, which the desktop app reads;
- the same human output, wording included, unless the user approves a change;
- the same state layout: `STATE_DIR`, slots, env files, PID files, logs;
- the same hooks and plugin behaviour.

The port runs in phases. Each phase keeps the shell fallback for the commands it has not
reached.

1. Read-only: `config`, `dash-project`, `dash`, `status`, `card`, `workflow`, `step` display,
   `gates`, `runs`, `mem` report.
2. Local actions: `new`, `up`, `down`, `ping`, `shared-down`, `cleanup`, `reap`, `prune`,
   `delete`, `complete`, `lane`, `label`, `write-set`.
3. Interaction: `approvals`, `scratch`, `run`, `menu`, `watch`, `app`.
4. Tickets: `ticket`, `jira`, and the sync watcher.
5. Setup: `setup`, and `config` writing.
6. Remove the shell implementation; `bin/ramify` execs the Rust binary.

## Acceptance criteria

AC1. `cli/` builds one `ramify` binary; ported commands use no shell or Python at runtime.

AC2. `dash --json` on this machine is byte-identical to the shell output and finishes under
2 seconds. Project work runs in parallel.

AC3. `dash-project --json` is byte-identical to the shell output on the reference fixtures and
finishes under 300ms.

AC4. Every command returns the same exit code as the shell for a success fixture, a usage
error and a failure fixture.

AC5. A worktree started by the shell is read and stopped by the Rust binary, and the reverse.
The slot, env and PID files keep their layout.

AC6. `bin/ramify` delegates ported commands and falls back for the rest. Tests cover both paths.

AC7. The plugin hooks keep working: status line, approvals window, session start, `reap`, and
`step` recording.

AC8. Phase 6 removes `libexec/*.sh`, `libexec/*.py` and the shell functions; `bin/ramify` execs
the binary.

AC9. A parity harness runs each command against fixtures and compares stdout, stderr and exit
code with the shell implementation.

## Out of scope

- New commands, or changed output unless the user approves it.
- The Electron app and its JSON contract.
- The vendored Python YAML package; the Rust binary uses a Rust YAML crate.
- The skills, hooks and work-order semantics.

## Open questions

- Wording: keep human output byte-identical through the migration, then improve it in a later
  branch? Proposed: yes.
- Distribution: build the binary on install and cache it, or commit a release binary?
  Proposed: build on install.
- The binary path for delegation: `cli/target/release/ramify`. Proposed: yes.

## Lane

Standard. The CLI contract and state layout stay the same; only the implementation changes.
Raise the lane if a phase must change the state layout or the JSON contract.
