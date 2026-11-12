# Review: step-commands

A separate reviewer agent read the diff, [diagnose.md](diagnose.md), [audit.md](audit.md) and
[verify.md](verify.md). It checked the routing, the exit codes and the guard port against
`workflow.py` and `native_guard.py`. Before the fixes below, it reproduced each finding marked
"verified" on a fixture.

## Findings and fixes

| # | Severity | Where | Failure scenario | Status |
|---|---|---|---|---|
| 1 | high | `cli/src/steps.rs` `yaml_scalar` | Verified. A note with a line break went into the frontmatter raw. The approvals window sends a multi-line note. `reject spec --note $'Redo\n---\nand'` ended the frontmatter early. After that, the order did not parse, `step gate --pr` exited 0 and `step guard` allowed edits: both failed open. | Fixed. Text with a line break or a control character is a double-quoted scalar with JSON escapes. Test: `a_multi_line_note_keeps_the_order_readable`. |
| 2 | medium | `cli/src/steps.rs` `Head::append_step`, `Head::fields` | Verified. `steps: [{…}]` lost its first entry on `step done`. An indented `steps:` list got a `- {…}` line at column 0, which made invalid YAML, and the PR gate failed open. | Fixed. `Head` parses the frontmatter. A changed key is written again from its parsed value, in PyYAML's style. A key that the line scan cannot find makes the command fail and changes nothing. `ask` is read from the parsed value. Test: `step_done_keeps_hand_written_steps`. |
| 3 | medium | `cli/src/workflow.rs` `auto_ask`, `opens_pr` | Verified. `auto_ask: yes` (true in YAML 1.1) made `load()` fail. The guard then allowed everything, and `step gate --pr` exited 2, which the hook lets through. | Fixed. `yaml11_bool` reads `yes`, `on`, `y` and `true` in their usual cases. Test: `auto_ask_yes_holds_edits`. |
| 4a | medium | `cli/src/guard.rs` `split_shell` | Verified, also in Python. In `cat x\ntouch src/x`, `touch` read as an argument of `cat`, so the call passed as a read. | Fixed. A line break ends a command, as `;` does. A `#` comment keeps its line break. Tests: `a_write_on_the_next_line_is_not_a_read` and the unit test `splits_like_shlex_with_punctuation`. |
| 4b | medium | `cli/src/guard.rs` `targets_root`, `shell_group` | Verified, also in Python. `cd .. && touch <worktree>/src/x` reads as a command outside the worktree. A script named `cat` outside the worktree passes as a read, because only the basename is checked. | Not fixed. It is a gap in the guard's design, carried over unchanged from `native_guard.py`, not a regression of the port. A fix tracks paths, not only the directory. Proposed: a separate bug. **Needs your waiver.** |
| 5 | low | `cli/src/steps.rs` `implicit` | Verified. `approve --note 1e3` made `step wait` print `approved: 1000.0`, because this CLI reads YAML 1.2. | Fixed. A note is quoted when YAML 1.2 does not read it back as a string. Test: `a_note_that_reads_as_a_number_stays_text`. |
| 6 | low | `cli/src/steps.rs` `wait` | Verified. `step wait --timeout inf` panicked with exit 101. | Fixed. An infinite or huge timeout waits for ever. Test: `wait_with_an_infinite_timeout_does_not_panic`. |
| 7 | low | `cli/src/steps.rs` `load` | `check_definitions` has no port: `gate: Human` becomes an auto gate, not an error. | Not fixed. `audit.md` already lists it. Proposed: the same follow-up as the repo-workflow route. **Needs your waiver.** |
| 8 | low | `cli/src/steps.rs` `load` | On an unborn branch, the branch name is empty. The shell printed `?`. | Not fixed. It changes only a message, on a repo with no commit. **Needs your waiver.** |

## Test gaps the reviewer named

- The parity sequence runs with `auto_ask` off. `auto_ask_yes_holds_edits` and
  `a_write_on_the_next_line_is_not_a_read` now cover the guard with it on, through the Rust
  binary.
- Not covered yet: digests with `auto_ask`, `ensure`, and `wait` timing out. The reviewer
  checked digests and `ensure` across Python and Rust by hand, and both held.

## Checks after the fixes

- `cd cli && cargo test`: 38 parity and integration tests pass, the 6 new ones included. 11 unit
  tests pass.
- With the fixes taken out (`git stash -- cli/src`), the 6 new tests fail.
- `cargo clippy --all-targets`: no warnings.
- `cargo fmt --check` reports differences in every file of the crate, on main too. The repo
  does not use rustfmt, so this branch does not run it.
