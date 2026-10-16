---
name: scratch
description: Hand the user a text to copy to the clipboard — a message to paste in Slack or an email, a command to run elsewhere, a ticket or PR body. It opens in the ramify scratch window with a Copy button. Use when the user asks for something to copy or paste, or for a scratchpad item.
---

# ramify scratch

Write the text to a file in your scratchpad, then hand it over:

```bash
"${CLAUDE_PLUGIN_ROOT}/bin/ramify" scratch --title "<what it is, for where>" <file>
```

- Pipe the text in instead of a file when it is short: `printf '%s' "$text" | ramify scratch --title …`.
- Put in exactly the text to paste: no preamble, no code fence around it.
- Give a title the user recognises in a tab: `Slack #team: deploy note`, `Jira PROJ-12 body`.
- The window runs detached. Do not wait on it and do not poll it. Report the line it prints.
- Run prose through the `ocre-jelly` skill before you hand it over.
- The first run installs Electron, as `ramify app` does. On a failure, give the text in the chat.
