# Ram Code providers

## Problem

Ram Code talks to one Anthropic-format endpoint, set by environment variables. To use another model, the user edits the environment and restarts. The user wants to switch between Claude, OpenAI, DeepSeek and the team's LiteLLM from inside Ram Code, as cc-switch does for Claude Code.

## Behaviour

A **provider** is a named model endpoint. It has a name, an API format, a base URL, the environment variable that holds its key, and a default model. Ram Code ships four presets:

| Name | Format | Base URL | Key | Default model |
| --- | --- | --- | --- | --- |
| `claude` | Anthropic Messages | `https://api.anthropic.com` | the order from #53: `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, then `ant auth login` | `claude-opus-5-5` |
| `openai` | OpenAI Chat Completions | `https://api.openai.com/v1` | `OPENAI_API_KEY` | see open question 2 |
| `deepseek` | Anthropic Messages | `https://api.deepseek.com/anthropic` | `DEEPSEEK_API_KEY` | `deepseek-v4.1-flash` |
| `env` | Anthropic Messages | `ANTHROPIC_BASE_URL`, else `http://127.0.0.1:15721` | `ANTHROPIC_AUTH_TOKEN` | today's model order |

`env` is today's behaviour. It stays the active provider until the user picks another, so nothing changes for a current setup.

The user adds custom providers, such as the team's LiteLLM, in `~/.config/ram-code/providers.json`. A custom provider has the same fields as a preset and either format. A custom entry with a preset's name overrides that preset. The file stores the name of the variable that holds the key, never the key itself.

The user switches providers in Ram Code:

- `/provider` lists the providers. It marks the active one and each one whose key is missing.
- `/provider <name>` makes `<name>` active. The choice is saved in `providers.json` and holds for later sessions.
- `/model <id>` sets the model for the active provider in this session.
- `RAM_CODE_PROVIDER` overrides the saved choice for one run. `RAM_CODE_MODEL` overrides the model.
- The status line shows the active provider and model.

For an OpenAI-format provider, Ram Code converts each request to Chat Completions and converts the reply back. Tool calls, tool results and text keep working. The agent loop does not change.

States:

- Empty: no `providers.json`. The presets are listed, and `env` is active.
- Error: an unknown name, a missing key, an unreadable `providers.json`, or an endpoint that fails. The message names the fix, and Ram Code stays usable.
- Success: the switch is confirmed in the transcript, and the next task uses the new provider.

## Acceptance criteria

AC1. Given no `providers.json` and no `RAM_CODE_PROVIDER`, when a task starts, then Ram Code uses `env` and sends the same request as before this branch.

AC2. Given `/provider`, then Ram Code lists `claude`, `openai`, `deepseek`, `env` and every custom provider. It marks the active one and each one whose key is missing.

AC3. Given `/provider deepseek`, when the next task starts, then the request goes to `https://api.deepseek.com/anthropic/v1/messages` with `DEEPSEEK_API_KEY` as `x-api-key`. A new Ram Code session also starts on `deepseek`.

AC4. Given a custom provider in `providers.json` with a base URL, a key variable, a model and a format, when it is active, then requests go to that base URL with that key and model.

AC5. Given an OpenAI-format provider and a task that calls a tool, then the request is a Chat Completions request with the tools as functions. The tool call runs, its result goes back as a `tool` message, and the final text shows in the transcript.

AC6. Given `/provider nope`, or an active provider whose key variable is unset, when the user acts, then Ram Code shows an error that names the fix, and the active provider does not change.

AC7. Given `RAM_CODE_PROVIDER=openai`, when Ram Code starts, then `openai` is active for that run, and `providers.json` keeps its saved choice.

AC8. `providers.json` never holds a key. Ram Code never writes a key to disk.

## Out of scope

- A routing proxy, failover, or a circuit breaker.
- Writing Claude Code's, Codex's or Gemini's config files (see open question 1).
- MCP, prompt and skill sync.
- The OpenAI Responses API, Gemini's native format, and streaming.
- Usage and quota tracking.

## Open questions

1. Should `/provider` also write the active provider into Claude Code's `~/.claude/settings.json`, as cc-switch does? This spec says no: it changes Ram Code only.
2. What is the default model for `openai`?
3. Is `~/.config/ram-code/providers.json` the right place, or should it live with ramify's own config?
4. What is the LiteLLM base URL, and does it take Anthropic or OpenAI format? If it is the `llms.nexapp.dev` endpoint, should it ship as a fifth preset?

## Lane

Standard. The branch handles credentials (tag `auth`) and adds a call to an external API with a different format.
