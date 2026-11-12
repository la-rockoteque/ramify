---
workflow: feature
lane: standard
branch: feat/ram-code-mcp
tags: [auth, permissions]
created: 2026-11-12
steps:
- {step: spec, date: '2026-11-12'}
- {step: qa-plan, date: '2026-11-12'}
- {step: build, date: '2026-11-12'}
- {step: audit, date: '2026-11-12'}
---

# Ram Code MCP

Ram Code becomes an MCP client. Local servers run over stdio inside the sandbox, and remote servers run over streamable HTTP with OAuth sign-in. Tools only. Project servers from `.mcp.json` start only after the human approves them. See [spec.md](spec.md).
