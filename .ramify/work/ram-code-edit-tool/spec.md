# Ram Code edit tool

## Problem

To change a file, Ram Code's model calls `write_file` with the whole new content. On a large file, that costs output tokens and time. A model that copies a long file can also drop or change lines it did not mean to touch. Claude Code and Codex give the model a targeted edit. This branch adds `edit_file`, which replaces one exact string in a file.

## Behaviour

**`edit_file`** replaces text in one file.

- Input: `path` (required), `old_string` (required), `new_string` (required), `replace_all` (default false).
- The path rules are the same as `read_file`. `Tools::resolve` keeps the path inside the worktree and rejects a symlink that leaves it.
- A path with a `.git` component is an error, as in the search tools and the sandbox.
- The file must exist, be UTF-8, have no NUL byte in its first 8 KiB, and be at most 10 MB.
- `old_string` must not be empty, and it must differ from `new_string`.
- Without `replace_all`, `old_string` must occur exactly once in the file.
  - No match is an error. It says so and suggests `read_file` or `grep` to see the current text.
  - More than one match is an error. It gives the count and the line of each match, at most 10. It asks for more context in `old_string`, or `replace_all`.
- With `replace_all`, every occurrence is replaced. At least one must exist.
- Matching is exact, byte for byte. No whitespace is trimmed and no line ending is changed.
  - A file with CRLF endings keeps them. When `old_string` and `new_string` use LF and the file uses CRLF, Ram Code converts both to CRLF before it matches. A file with mixed endings is matched as it is.
  - The file keeps its trailing newline, or keeps its lack of one.
- Ram Code writes the result to a temporary file in the same folder, then renames it over the original. A failed write never leaves half a file. The file's permissions stay the same.
- Output: one line, such as `edited src/app.rs: replaced 1 occurrence, now lines 120–134`. With `replace_all`, it gives the count and the first and last line. It never echoes the file.

An empty `old_string` does not create a file. `write_file` creates files. One tool for each job keeps the errors clear, and an `edit_file` call can never overwrite a file by mistake.

**`write_file` stays.** It creates new files and does full rewrites. Its description says to use `edit_file` for a change to an existing file.

**The system prompt** adds one sentence: change an existing file with `edit_file`, and use `write_file` only for a new file or a full rewrite.

Both provider formats get `edit_file` through `Tools::definitions`.

States:

- Error: a path outside the worktree or under `.git`, a missing, binary, non-UTF-8 or oversized file, an empty `old_string`, `old_string` equal to `new_string`, no match, or more than one match without `replace_all`. The model gets the error as the tool result. The file does not change, and the loop goes on.
- Success: the one-line confirmation.

## Acceptance criteria

AC1. Given a file where `old_string` occurs once, when the model calls `edit_file`, then only that text changes, and the rest of the file is byte for byte the same.

AC2. Given `old_string` that occurs three times and no `replace_all`, then the tool returns an error with the count 3 and the three line numbers, and the file does not change.

AC3. Given the same file and `replace_all: true`, then all three occurrences change, and the output gives the count 3.

AC4. Given `old_string` that does not occur, then the tool returns an error that suggests `read_file` or `grep`, and the file does not change.

AC5. Given an empty `old_string`, or `old_string` equal to `new_string`, then the tool returns an error, and the file does not change.

AC6. Given a file with CRLF endings and an LF `old_string` and `new_string`, then the edit applies, and every line of the file still ends in CRLF.

AC7. Given a file with no trailing newline, and an edit on its last line, then the file still has no trailing newline. Given a file with one, it keeps it.

AC8. Given a path outside the worktree, through a symlink that leaves it, or under `.git`, then the tool returns an error and writes nothing.

AC9. Given a missing file, a binary file, a non-UTF-8 file, or a file over 10 MB, then the tool returns an error, and the file does not change.

AC10. Given an executable file, then it is still executable after the edit.

AC11. Given a successful edit, then the output is one line with the new line range, and it does not hold the file's content.

AC12. The Anthropic request and the Chat Completions request both list `edit_file`. The system prompt and the `write_file` description steer changes to `edit_file`.

## Out of scope

- Several edits in one call, as in Claude Code's MultiEdit.
- Patch formats, such as Codex's `apply_patch` or unified diffs.
- Notebook edits.
- A check that the model read the file before it edits it.
- Fuzzy or whitespace-tolerant matching.

## Open questions

None.

## Lane

Standard. The tool writes to the user's files, and the workflow puts every feature on the standard lane unless the user picks the light lane.
