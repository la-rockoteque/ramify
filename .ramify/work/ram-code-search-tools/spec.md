# Ram Code search tools

## Problem

Ram Code's model can only list one folder at a time and read whole files. To find a function or a string, it reads file after file, or runs `grep` and `find` through the shell. That costs turns and tokens. Claude Code and Codex give their models search tools and a structure view. This branch adds three read-only tools: `grep`, `glob` and `code_map`.

## Behaviour

All three tools are read-only and stay inside the worktree, like `read_file`. They skip what `.gitignore` ignores, the `.git` folder, and binary files. They do not follow a symlink out of the worktree. Each one has an output limit and says when it cut the output.

**`grep`** searches file contents with a regular expression.

- Input: `pattern` (required), `path` (a file or folder, default `.`), `glob` (a file filter such as `*.rs`), `ignore_case` (default false).
- Output: one line per match, `path:line: text`, sorted by path and line. At most 200 matches. A matched line longer than 300 characters is cut.
- No match gives `no matches`. A bad regular expression gives an error that quotes the parser's message.

**`glob`** finds files by name.

- Input: `pattern` (required, such as `src/**/*.rs`), `path` (the folder to search from, default `.`).
- Output: one relative path per line, sorted by path. At most 500 paths.
- No match gives `no files`. A bad pattern gives an error.

**`code_map`** shows the structure of code without its bodies.

- Input: `path` (a file or folder, default `.`).
- Output: per file, its path, then one line per symbol: kind, name and line number. Nested symbols are indented under their parent, such as methods under an `impl` or a class.
- It parses with tree-sitter. The first languages are Rust, TypeScript and JavaScript (`.ts`, `.tsx`, `.js`, `.jsx`), Python and Java.
- A file in another language is listed with its line count only.
- At most 40,000 characters, cut at a file boundary.

The system prompt names the tools, so the model uses them before it reads whole files.

States:

- Empty: `no matches`, `no files`, or a file with no symbols.
- Error: a path outside the worktree, a bad pattern, a file that does not parse. The model gets the error as the tool result, and the loop goes on.
- Success: the output described above.

## Acceptance criteria

AC1. Given a worktree with a match in a tracked file and in an ignored file, when the model calls `grep`, then only the tracked match comes back, as `path:line: text`.

AC2. Given `grep` with `glob: "*.rs"` and `ignore_case: true`, then only `.rs` files are searched, and case does not matter.

AC3. Given more than 200 matches, then `grep` returns 200 and says the output was cut.

AC4. Given a bad regular expression, or a `path` outside the worktree or through a symlink that leaves it, then `grep`, `glob` and `code_map` return an error and read nothing.

AC5. Given `glob` with `src/**/*.rs`, then it returns every `.rs` file under `src` that `.gitignore` does not ignore, sorted.

AC6. Given a Rust file with a struct, an impl with two methods, and a free function, when the model calls `code_map` on it, then each symbol comes back with its kind, name and line, and the methods are indented under the impl.

AC7. Given a folder with Rust, TypeScript, Python, Java and Markdown files, then `code_map` outlines the first four and lists the Markdown file with its line count.

AC8. Given a binary file, then `grep` skips it and `code_map` does not list it.

AC9. The OpenAI-format and Anthropic-format providers both receive the three new tool definitions.

## Out of scope

- Editing tools (an exact-string edit tool comes in a later branch).
- Languages beyond the four above.
- Ranking or summarising files, as in aider's repo map.
- Semantic or embedding search.

## New dependencies

`ignore` (gitignore-aware walking, from ripgrep), `globset`, `regex`, `tree-sitter`, and the grammars `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-javascript`, `tree-sitter-python` and `tree-sitter-java`. The grammars compile C code at build time.

## Open questions

None. The user chose a tree-sitter symbol outline for `code_map`.

## Lane

Standard. The branch adds new dependencies, including C grammar code built at compile time.
