# Spec: Ram Code reads AGENTS.md and CLAUDE.md

## Problem

Codex reads `AGENTS.md`, and Claude Code reads `CLAUDE.md`: a project writes its conventions
there once, and each agent follows them. Ram Code reads neither, so it ignores the rules that
the other two agents follow in the same worktree. The ramify repository also has no such file
for any agent.

## Behaviour

1. Before each task, Ram Code reads the instruction files below, in this order. Each file that
   exists is used. A missing file is skipped with no message.
   1. `~/.ram/AGENTS.md`, then `~/.claude/CLAUDE.md`: the user's own rules.
   2. The project's files, in each folder from the git top level of the worktree down to the
      folder Ram Code works in. In each folder: `AGENTS.md`, then `CLAUDE.md`, then
      `.claude/CLAUDE.md`, then `CLAUDE.local.md`.
2. A file that two names reach (a link from `CLAUDE.md` to `AGENTS.md`, for example) is read
   one time, at its first place in the order. Ram Code compares the real paths.
3. In a `CLAUDE.md` file, a line part `@path` imports another file, as in Claude Code:
   - The path is relative to the folder of the file that imports it. `~/` is the home folder.
   - An import inside a code span or a code block is not an import.
   - An imported file can import other files, to a depth of 5.
   - A file that is already read is not read again, so a loop ends.
   - An import of a missing file stays as text.
   - The imported text replaces the `@path` word in the file.
4. Each file is cut at 32 KiB, with the note `[cut at 32 KiB]`. All the files together are cut
   at 128 KiB. A file that is not UTF-8 is skipped with one transcript line.
5. The system prompt is Ram Code's own prompt, then one section per file:
   `Instructions from <path>:` and the text. The path is relative to the worktree for project
   files, and starts with `~/` for the user's files. The order is the order of 1, so a later,
   more specific file comes last and can refine an earlier one.
6. The same system prompt goes to every provider: the Anthropic API, and as the system message
   of an OpenAI-style provider.
7. `/instructions` in the interface lists the files the next task reads, in order, with their
   size. With no file, it says `no AGENTS.md or CLAUDE.md`.
8. `RAM_CODE_INSTRUCTIONS_HOME` replaces `~` for the user's files. The tests set it.
9. The ramify repository gets an `AGENTS.md` with its conventions: the two crates, the build
   and test commands, the ramify workflow, the prose rules, and the write rules for commits.
   It also gets a `CLAUDE.md` that holds only `@AGENTS.md`, so Claude Code reads the same rules.

## Acceptance criteria

- AC1: Given `~/.ram/AGENTS.md` and `~/.claude/CLAUDE.md`, then the system prompt has both
  texts, `AGENTS.md` first.
- AC2: Given `AGENTS.md`, `CLAUDE.md`, `.claude/CLAUDE.md` and `CLAUDE.local.md` in the
  worktree, then the system prompt has the four texts in that order, after the user's files.
- AC3: Given a worktree folder below the git top level, with an `AGENTS.md` in each, then the
  top-level file comes before the file of the folder below.
- AC4: Given `CLAUDE.md` as a link to `AGENTS.md`, then the text is in the system prompt one
  time.
- AC5: Given a `CLAUDE.md` with `@docs/rules.md`, then the system prompt has the text of
  `docs/rules.md` in its place. Given the same `@path` in a code span or a code block, then it
  stays as written.
- AC6: Given two files that import each other, then each text is in the prompt one time, and the
  task starts.
- AC7: Given an import of a missing file, then `@path` stays as text.
- AC8: Given a file of 40 KiB, then the prompt has its first 32 KiB and the note
  `[cut at 32 KiB]`.
- AC9: Given no instruction file, then the system prompt is Ram Code's own prompt, unchanged.
- AC10: Given an OpenAI-style provider, then its system message has the instruction texts.
- AC11: `/instructions` lists the files in order with their sizes, or says
  `no AGENTS.md or CLAUDE.md`.
- AC12: The ramify repository has `AGENTS.md` and a `CLAUDE.md` that imports it. Ram Code reads
  the text of `AGENTS.md` one time.
- AC13: The README documents the files, the order, the imports, the limits and
  `/instructions`.

## Amendment: review (2026-11-12)

10. A project file, and each file it imports, must have a real path inside the git top level.
    Ram Code refuses `@~/…`, `@/…` and an import that leaves the project, and a project file that
    links outside it. Each refusal leaves the text as written and adds one transcript line. The
    user's own files may import from anywhere. Claude Code asks before such imports; Ram Code has
    no dialog, so it refuses them.
11. A project file is named relative to the git top level, even when the repository is in the
    home folder.
12. Each file is cut at 32 KiB when it is read, so an import has its own limit. For the total,
    the section that crosses 128 KiB is cut and the later sections are left out, with a note.
    `/instructions` shows what the limits did to each file.
13. With no home folder, Ram Code reads only the project's files.
14. `/instructions` reads the files on a thread. The other test files point
    `RAM_CODE_INSTRUCTIONS_HOME` at a temp folder.

## Out of scope

- Files in subfolders below the folder Ram Code works in, which Claude Code reads when it opens
  a file there.
- `~/.codex/AGENTS.md`, Codex's override files (`AGENTS.override.md`), and Codex's
  `project_doc_fallback_filenames`.
- Imports in `AGENTS.md`: Codex has none.
- An interface to edit the files, such as Claude Code's `/memory` or `#` shortcut.

## Open questions

None. The user chose `AGENTS.md` and `CLAUDE.md` both, with an `AGENTS.md` for the ramify
repository.

## Lane

Standard. The files change what every task sends to the model, and the repo gets new agent
rules.
