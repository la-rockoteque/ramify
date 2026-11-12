# Ram Code MCP

## Problem

Ram Code's model has only its built-in tools. Claude Code and Codex connect to MCP servers, which give the model tools for issue trackers, databases, browsers and internal APIs. Teams already describe these servers in a project's `.mcp.json`. This branch makes Ram Code an MCP client: local servers over stdio, remote servers over streamable HTTP, and OAuth sign-in for remote servers. Tools only at first.

## Behaviour

### Configuration

Ram Code reads two files:

1. The user file, `~/.config/ram-code/mcp.json` (under `RAM_CODE_CONFIG_DIR` when it is set).
2. The project file, `.mcp.json` at the git top level of the worktree.

Both use Claude Code's format:

```json
{
  "mcpServers": {
    "tracker": { "command": "npx", "args": ["-y", "tracker-mcp"], "env": { "TRACKER_URL": "${TRACKER_URL}" } },
    "docs":    { "type": "http", "url": "https://mcp.example.com/mcp", "headers": { "X-Team": "core" } }
  }
}
```

- An entry with `command` is a stdio server. An entry with `type: "http"` and `url` is a remote server. Any other entry is an error for that server only.
- On a name clash, the user file wins, because a project file must not replace a server the user trusts.
- `${VAR}` and `${VAR:-default}` expand from Ram Code's environment, in `command`, `args`, `env`, `url` and `headers`. An unset variable with no default is an error for that server.
- A `headers` value may name a secret only through `${VAR}`. Ram Code never writes an expanded value to disk or to the transcript.

### Approval of project servers

A repository must not run commands, or reach a URL with the user's sign-in, just because Ram Code opened it.

- A server from `.mcp.json` does not start until the human approves it. User-file servers need no approval.
- At the first task, the TUI shows one prompt per unapproved project server. It shows the name, the full command line or URL, the names of the `env` and `headers` keys, and the sandbox choice. The keys are:
  - `a`: approve, inside the sandbox with no network (the default).
  - `n`: approve, inside the sandbox with network.
  - `u`: approve, outside the sandbox. The prompt shows this choice in red.
  - `d`: deny.
- Ram Code stores the decision in `~/.config/ram-code/mcp-approvals.json`, keyed by the worktree's git top level and a SHA-256 of the server's entry. A changed entry asks again.
- `/mcp approve <name>` and `/mcp deny <name>` change a decision later.
- A headless run (`--task`) never prompts. It skips unapproved project servers and prints one line for each.

### Lifecycle

- Each task runs in its own worker process, so servers start in the worker when the task starts. They start in parallel. A server that is not ready within 30 seconds is marked failed for the task, and the task goes on without it.
- Ram Code sends `initialize` with its name and version, waits for the result, sends `notifications/initialized`, then calls `tools/list`. It follows `nextCursor` until the list is complete.
- A `tools/call` waits at most 120 seconds. A server can lower or raise this with `"timeout_seconds"` in its entry, to at most 600.
- When a stdio server exits during a task, the next call to one of its tools starts it once more. A second crash in the same task marks it failed.
- At the end of the task, Ram Code closes each stdio server's input, waits 2 seconds, then kills its process group. It closes HTTP sessions with a `DELETE` when the server gave a session id.
- `/mcp` lists each server: source (user or project), transport, approval, sandbox choice, state, tool count and the last error. The panel's Agents pane shows the same states for the running task.

### Tools for the model

- Each MCP tool appears as `mcp__<server>__<tool>`. A name with characters outside `[A-Za-z0-9_-]` has them replaced with `_`. A name longer than 64 characters is cut, with a short hash to keep it unique.
- The tool's description and `inputSchema` pass through unchanged to both formats: `input_schema` for Anthropic Messages, `function.parameters` for OpenAI Chat Completions.
- A result becomes text:
  - Text content is kept as it is.
  - An image or audio item becomes `[image: <mime type>, <size> bytes]` or `[audio: …]`.
  - A resource link becomes `[resource: <uri>]`.
  - `structuredContent` is added as JSON when the server gives no text.
  - `isError: true` gives `error: …` to the model.
- Each result is cut at 40,000 characters, with a note.
- MCP calls go through the same `PreToolUse` and `PostToolUse` hooks as built-in tools (`run_tool_hooked` in `src/agent.rs`). The hook sees the `mcp__…` name, so a hook can deny one server or one tool.

### Security

- A stdio server runs inside the #62 sandbox by default. It writes only in the worktree and its own temp folder, and it has no network.
- Network, or running outside the sandbox, needs the human's choice at approval (`n` or `u`). The choice is stored per server.
- The sandbox with network keeps every other rule: no credential files, no `.git` writes, no terminal, no launches outside.
- The server's environment is scrubbed as for `run_command`. The variables its entry names in `env` are added back after the scrub, so a server can get the one key it needs.
- A remote server's URL must use `https`. `http` is allowed only to `localhost`, `127.0.0.1` and `[::1]`.

### OAuth for remote servers

- When a remote server answers `401` with a `WWW-Authenticate` header, Ram Code follows the MCP authorization spec:
  1. It reads the protected-resource metadata, then the authorization server's metadata.
  2. It registers a client with dynamic client registration, when the server supports it.
  3. It runs the authorization code flow with PKCE (S256) and the `resource` parameter.
- The browser opens on the authorization URL. Ram Code waits for the redirect on a loopback port (`http://127.0.0.1:<port>/callback`) and checks `state`.
- Ram Code stores the access token, refresh token, expiry and client registration in the macOS Keychain, under the service `ram-code-mcp` and the server's URL. The ramify CLI uses the same `security` tool for Jira tokens. On Linux, the secret service is used through `secret-tool` when it exists. With no secret store, tokens live in memory for the session only.
- Ram Code refreshes an expired token once before a call. A failed refresh asks for sign-in again.
- `/mcp login <name>` signs in, and `/mcp logout <name>` deletes the stored tokens.
- The model, the transcript, the events file and the hooks never see a token or a header value.

### States

- Empty: no config file. `/mcp` says how to add a server, and the task runs with built-in tools only.
- Starting: the server is launching or initializing. `/mcp` shows `starting`.
- Ready: `/mcp` shows the tool count.
- Needs approval: a project server waits for the human. Its tools are not offered.
- Failed: the command was not found, the server crashed twice, the handshake timed out, or the config was bad. `/mcp` and the transcript show the error, and the task goes on.
- Auth required: a remote server answered `401`. The transcript says `/mcp login <name>`. Its tools are not offered until sign-in.
- Auth failed: the browser flow was cancelled, `state` did not match, or the token exchange failed. The error names the step.
- Tool error: the model gets `error: …` and the loop goes on.

## Acceptance criteria

AC1. Given a user `mcp.json` with a stdio server and a test server binary, when a task starts, then the model's tools include `mcp__<server>__<tool>`. A call returns the server's text result.

AC2. Given the same server name in the user file and in `.mcp.json`, then the user file's entry is used.

AC3. Given `${VAR}` and `${VAR:-default}` in `command`, `args`, `env`, `url` and `headers`, then they expand. An unset variable with no default fails that server only, and the error names the variable.

AC4. Given an unapproved server in `.mcp.json`, when the first task starts in the TUI, then the server does not start until the human answers the prompt. `d` keeps it stopped, and its tools are never offered.

AC5. Given an approved project server whose entry then changes, when a task starts, then Ram Code asks again. A headless run skips the unapproved server and prints one line.

AC6. Given a stdio server approved with `a`, then it cannot write outside the worktree and its temp folder, and it cannot open a network connection. With `n`, it can reach the network, and every other sandbox rule still holds.

AC7. Given `OPENAI_API_KEY` in Ram Code's environment and an entry whose `env` names `TRACKER_TOKEN`, then the server sees `TRACKER_TOKEN` and not `OPENAI_API_KEY`.

AC8. Given a server that answers `tools/list` in two pages, then all its tools are offered. Given a tool name with spaces, or longer than 64 characters, then the offered name is valid and unique.

AC9. Given a result with text, an image, a resource link and `isError: true`, then the model gets the text, the placeholders and `error: …`. A result over 40,000 characters is cut with a note.

AC10. Given a `tools/call` that never answers, then the call returns an error after the timeout, and the task goes on.

AC11. Given a stdio server that crashes, then the next call restarts it once. A second crash marks it failed. No server process is left running after the task ends.

AC12. Given a `PreToolUse` hook that denies `mcp__tracker__delete_issue`, then that call does not reach the server, and the model gets the hook's reason.

AC13. Given a remote server at an `http` URL that is not loopback, then Ram Code refuses it with an error and makes no request.

AC14. Given a remote test server that answers `401` with resource metadata, a test authorization server with dynamic registration, and an automated browser step, when the human runs `/mcp login <name>`, then Ram Code registers, uses PKCE and checks `state`. It stores the tokens in the secret store, and later calls send `Authorization: Bearer`.

AC15. Given an expired access token and a valid refresh token, then Ram Code refreshes once and the call succeeds. Given a failed refresh, the server shows `auth required`.

AC16. Given any run with a signed-in remote server, then no token, header value or expanded secret appears in the transcript, the events file, the hook input or any file Ram Code writes.

AC17. Given no MCP config, then `/mcp` shows the empty state, and tasks run as before, with the same tool list.

AC18. Both provider formats receive the MCP tools: the Anthropic request's `tools` and the Chat Completions `tools`.

## Scope of this branch

The user chose two branches. This branch delivers AC1–AC12, AC17 and AC18: configuration, approval, stdio servers in the sandbox, naming, hooks, `/mcp`, and both provider formats. AC13–AC16 (streamable HTTP, OAuth, tokens in the Keychain) move to a second branch, `feat/ram-code-mcp-oauth`, with its own spec. In this branch, an `http` entry in the configuration is listed by `/mcp` as `not supported yet` and offers no tools.

## Out of scope

- MCP resources, prompts, sampling, roots, elicitation and logging.
- The legacy SSE transport (HTTP+SSE from the 2024-11-05 spec).
- Server-initiated requests and notifications, apart from `tools/list_changed`, which this branch ignores.
- OAuth flows other than authorization code with PKCE, such as client credentials or device code.
- A shared server process across tasks or sessions.
- Windows.

## Dependencies

Recommendation: the official Rust SDK, `rmcp`, with the features for the client, the child-process transport, the streamable HTTP client transport and auth.

- It follows the protocol revisions, which change often.
- It includes OAuth with PKCE, dynamic client registration and metadata discovery. That part is the riskiest to write by hand.
- The cost: `rmcp` is async, so it brings `tokio` and `reqwest` (with `rustls`), and `oauth2` with the auth feature. Ram Code is synchronous today (`ureq`). The worker would run one current-thread `tokio` runtime for MCP only. The build gets slower, and the dependency tree grows by roughly 100–150 crates.

Alternative: a small synchronous client on `ureq` and `serde_json`. The stdio and HTTP protocol subset is about 400 lines. OAuth with discovery, registration and PKCE adds about 500 more, with `sha2` and `base64`. That means fewer dependencies, but more code to keep correct against the spec.

The Keychain uses the `security` command, as `cli/src/jira.rs` does. Linux uses `secret-tool`. There is no keyring crate.

## Decisions

The user settled the open questions on 2026-11-12.

1. Two branches, as described in "Scope of this branch".
2. Ram Code uses the official `rmcp` SDK, with its `tokio` and `reqwest` dependencies in the worker.
3. Servers start again for each task, because each task runs in a new worker. Keeping servers alive in the TUI process is a later change.
4. The approval prompt suggests `n` (network) when the command is `npx` or `uvx`, and says why. The human still chooses.

## Lane

Standard, tags `auth` and `permissions`. The branch runs commands from repository files, signs in to remote services, and stores tokens.
