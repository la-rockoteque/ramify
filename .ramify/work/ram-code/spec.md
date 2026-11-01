# Ram Code

## Problem

The Ramify repository has a desktop dashboard for managing worktrees and QA stacks. The user wants a coding harness they can operate from a terminal. Ram Code brings Ramify operations into a terminal interface and hands coding tasks to DeepSeek in a selected worktree. Claude Opus coordinates by default, DeepSeek does delegated work through `llms.nexapp.dev`, and Codex can be added later as another provider.

## Behaviour

Ram Code is a Rust terminal application. It uses Ratatui with Crossterm for rendering and input. On launch it shows the current project, its worktrees and their workflows, read from the Ramify CLI. The user inspects a worktree and invokes Ramify actions from the terminal.

A coding screen takes a task for the selected worktree. Ram Code sends the task to DeepSeek through the Anthropic-compatible Nexapp endpoint. The model reads files, writes files and runs commands in that worktree. The screen shows each step and the final message. Credentials come from `ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN`. Ram Code stores no key.

The screen is a chat that keeps running. The transcript shows the user's task, the model's text, and each tool call with its result, in the style of Claude Code. After a task ends, the transcript stays and the next task carries the earlier turns to the model. Slash commands run the Ramify actions: `/up`, `/down`, `/step`, `/status`, `/new <slug>`, `/worktrees`, `/refresh`, `/clear`, `/help`, `/quit`. `Tab` opens the worktree picker.

Each state is visible:

- Empty state names the next step to configure a project.
- Loading state says Ramify state is being read.
- Error state shows the failed operation and keeps retry and quit available.
- Success state reports the result and refreshes the view.

The first Ramify actions are inspect status, create a worktree, start or stop its QA stack, and show its current workflow step.

## Acceptance criteria

AC1. Given a configured project, when Ram Code starts, then it shows the project and its worktrees, read from Ramify.

AC2. Given a worktree in the list, when the user selects it, then Ram Code shows its branch, dirty state, services and workflow step.

AC3. Given a supported action, when the user invokes it, then Ram Code runs the Ramify CLI with that worktree as root and shows the output.

AC4. Given no project state, when Ram Code starts, then it shows an empty state with a next step. Given unreadable state, it shows an error state. Neither case crashes.

AC5. Given a Ramify command that exits nonzero, when it returns, then Ram Code shows its output and the user can return to the list.

AC6. Ram Code runs without Electron.

AC7. Given a selected worktree and a task, when the user starts the task, then Ram Code sends it to DeepSeek with file and command tools rooted at that worktree, applies the tool calls, and shows progress and the final message.

AC8. Given a missing token, when the user starts a task, then Ram Code shows the fix and the interface stays usable.

AC9. Given a finished task, when the user sends another task, then the transcript stays visible and the model receives the earlier turns.

## Out of scope

- Implementing Ramify in Rust. Ram Code calls the CLI.
- Claude and Codex as providers. DeepSeek is first.
- Credential storage.
- Parallel workers and agent policy.
- The Electron app in `app/`.
- Changes to Ramify CLI behavior.

## Lane

Standard lane. The feature adds a terminal app and calls one API. It changes no Ramify contract and no protected data.
