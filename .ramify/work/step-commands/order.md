---
workflow: bug
lane: standard
branch: fix/step-commands
tags: []
created: 2026-11-12
steps:
- {step: diagnose, date: '2026-11-12'}
- {step: fix, date: '2026-11-12'}
- {step: audit, date: '2026-11-12', note: binary rebuild waits on a release}
---

# step-commands

`ramify step done|na|gate|ask|wait|guard|ensure`, `approve` and `reject` print the usage and exit 2.
Commit `525feea` deleted the bash CLI before the Rust port had these commands. No workflow can
record a step, and the PR gate and the edit guard fail open. See [diagnose.md](diagnose.md).
