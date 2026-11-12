# ramify: rules for coding agents

ramify gives each branch its own git worktree and QA stack. This repository holds two Rust
crates, the Claude Code and Codex plugin, and a desktop app.

## Layout

- `src/`, `Cargo.toml`: Ram Code, the terminal harness (crate `ram-code`, Ratatui).
- `cli/`: the `ramify` CLI (crate `ramify-cli`). `src/` includes `cli/src/hooks.rs` with
  `#[path]`, so that file may use only `std`, `libc`, `serde_json` and `regex`.
- `bin/ram`, `bin/ramify`: the launchers. They run the release build, or build it.
- `hooks/`: `ramify-hook` (bash), and the hook files for Claude Code (`.claude-plugin/plugin.json`),
  Codex (`hooks/codex.json`) and Ram Code (`hooks/ram.json`).
- `skills/`: one folder per workflow step. `workflows.yml` names the steps and lanes.
- `app/`: the Electron desktop app (approvals, run and scratch windows).
- `shell/`: the zsh prompt plugin and the powerlevel10k segment.

## Checks

Run these before you record a build or a review step:

```bash
cargo test                                   # Ram Code
cargo test --manifest-path cli/Cargo.toml    # the ramify CLI
cargo clippy --all-targets                   # in the root and in cli/: no warning
bin/ramify write-set .ramify/work/<slug>/qa-plan.md
```

- `prompt_is_fast_with_twenty_worktrees` can fail under a full parallel run. Run it alone
  before you call it a failure.
- `test/smoke.sh` runs for minutes. `python3 test/skills.py` needs PyYAML.
- A test must never read or write the user's own files. Point `HOME`-like variables
  (`RAMIFY_HOOKS_HOME`, `RAM_CODE_INSTRUCTIONS_HOME`, `RAM_CODE_CACHE_DIR`,
  `RAMIFY_STATE_DIR`) at a temp folder. Keep the real `HOME`: hook scripts need it.

## Workflow

Every change goes through a ramify work order (`.ramify/work/<slug>/order.md`) on its own
branch and worktree. `ramify step` names the next step and its skill.

- `spec`, `qa-plan` and `review` are human gates. Ask with `ramify step ask <step>`, then wait
  with `ramify step wait <step>`. Never pass `--approved` without an explicit OK from the user.
- Name each test as the QA plan names it, and keep the diff inside the plan's write set.
- A review runs in a fresh agent that did not write the code. Fix every finding, or get the
  user's waiver and write it down.
- Never merge a pull request. The user merges. `finale` runs after the merge.

## Prose

All text is English in ASD-STE100 (Simplified Technical English): docs, comments, commit
messages, PR bodies and CLI messages. Pass it through the `ocre-jelly` skill when it is
installed.

- One idea per sentence. A procedure sentence has 20 words at most, a description 25.
- Use the active voice and simple tenses. No idiom, no filler, no recap.
- A comment says the non-obvious why, and nothing else.

## Commits

- Write the subject in the imperative, with no type prefix: `Find and install Ram Code updates`.
- End the message with the agent's `Co-Authored-By:` line.
- Commit in small steps. Never rewrite a pushed branch.
