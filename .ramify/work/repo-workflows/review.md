# Repository workflow review

The independent reviewer checked the spec, QA plan, QA evidence, full diff, and new helper and tests.

## Findings and fixes

1. P2: A preserved Ramify order on the current branch could force a new native task to use the default branch prefix.
   Branch calculation now uses the intended new task's context.
   The regression preserves an old order, starts a new native worktree, and confirms the configured `story/` prefix.

2. P2: Product workflow documentation could select repository mode through incidental development, review, or testing language.
   Discovery now requires a software-process heading or code-delivery evidence alongside process signals.
   The equipment workflow regression retains Ramify defaults.
   Linked CI configuration is also excluded from documentation discovery.

The reviewer confirmed both fixes and found no remaining actionable findings.

## Validation

- 15 repository workflow tests pass, covering setup, source precedence, native records, worktrees, CLI output, and Claude/Codex hooks.
- All 6 Codex tests and 21 Jira lifecycle tests pass.
- Smoke passes 187 cases with two documented baseline terminal-colour failures.
- Write-set, shell syntax, and whitespace checks pass.
- A read-only setup draft against Moship's actual worktree selects its five pipeline documents.

Moship's files were not changed.
Ramify follows the native workflow through its guides and skills; it does not translate arbitrary Markdown into executable steps.
