# Use the repository's workflow

## Problem

Some repositories already define their delivery workflow in documentation and local skills.
Ramify currently uses its defaults unless the repository provides `.ramify/workflows.yml`.
This introduces competing steps and duplicate work records in projects such as Moship.

## Behaviour

During setup, inspect repository instructions and development workflow documents.
Detect pipeline documents such as Moship's `docs/feature-pipeline.md`, `docs/bug-fix-pipeline.md`, `docs/tweak-pipeline.md`, `docs/spike-pipeline.md`, and `docs/release-pipeline.md`.
Also recognize explicit workflow guides linked from repository instructions and conventional development workflow locations.
Distinguish development process documentation from product workflows such as equipment fulfillment.

When setup finds an existing development workflow, select it automatically in the draft configuration.
Show the source paths and why they were selected.
An existing `.ramify/workflows.yml` remains the explicit structured override.
Repositories without a detected workflow retain Ramify's default workflows.
Allow an explicit configuration choice between repository documentation and Ramify definitions.
Setup must preserve an existing configuration unless the user requests replacement.
`setup --print` reports the selection without writing files.

In repository mode, the repository's documentation, skills, gates, artifact locations, and step records remain authoritative.
`ramify workflow` shows the selected documents and available named tracks when detection can identify them.
Starting a worktree keeps the project's branch conventions and points to the selected track.
Do not create a parallel `.ramify/work/<slug>/order.md` in this mode.
`ramify step`, session startup, and resume guidance direct the agent to the repository workflow and its existing work item.
Do not replace native steps with `ramify:spec`, `ramify:build`, or other default step skills.

For Moship, keep the existing `docs/stories`, `docs/bugs`, `docs/tweaks`, `docs/spikes`, and `docs/releases` records.
Its pipeline documents define how to locate the item and determine its next step.
Ramify must not guess a step from arbitrary prose or rewrite a native `steps:` block into its own schema.
Commands that would mutate a Ramify work order explain how to follow the repository workflow instead.
Ramify's PR gate must not demand its default checklist in repository mode; existing repository hooks remain responsible for native workflow gates.
Git safety, worktrees, QA stacks, and Jira integration remain available.

If discovery finds multiple conflicting guides, show the candidates and require an explicit selection rather than inventing a workflow.
If a configured guide disappears or cannot be read, report that error instead of silently reverting to the default workflow.
Worktrees resolve the same relative sources from their own checkout.
Existing Ramify work orders remain usable after setup selects a repository workflow; do not migrate or discard unfinished records.

## Acceptance criteria

AC1. Setup detects Moship's five delivery pipeline documents and selects the repository workflow automatically.

AC2. Setup reports the chosen source paths and preserves the read-only behaviour of `--print`.

AC3. Repository mode routes workflow, worktree startup, status, and session guidance to the project's documents and skills.

AC4. Repository mode creates no duplicate Ramify work order and does not impose Ramify's default checklist or rewrite native records.

AC5. Explicit structured definitions and explicit source selections take precedence over detection. Repositories without workflow evidence retain the defaults.

AC6. Domain workflow documents alone do not trigger development workflow detection. Conflicting or missing sources produce an actionable result.

AC7. Worktrees use checkout-relative sources, and an existing Ramify work order can finish without a migration.

AC8. Existing setup, native Ramify workflows, approval gates, worktrees, Jira synchronization, and Codex handoff retain their tested behaviour.

## Scope and lane

Standard lane: this changes setup, workflow routing, and session instructions.
This feature connects Ramify to an existing workflow. It does not translate arbitrary Markdown into executable workflow definitions.
Moship is a read-only reference and regression fixture source. Do not change Moship's files or run setup there during this feature.

## Reference evidence

Moship's current primary checkout is behind its remote main. The pipeline documents exist in its worktrees and remote main tree.
The feature pipeline defines native records under `docs/`, per-story branches, its own skills, and optional tracker mirrors.
`docs/orchestration.md` delegates to those existing tracks without redefining their semantics.
