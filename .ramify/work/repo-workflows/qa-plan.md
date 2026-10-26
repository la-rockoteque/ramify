# Repository workflow QA plan

Use temporary repositories and sanitized documentation fixtures. Do not modify Moship.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `test_setup_detects_moship_pipeline_documents` | build |
| Q2 | AC2 | medium | `test_setup_print_reports_sources_without_writes` | build |
| Q3 | AC3 | high | `test_repo_workflow_routes_cli_and_session_guidance` for Claude and Codex | build |
| Q4 | AC4 | high | `test_new_uses_repo_workflow_without_duplicate_order` | build |
| Q5 | AC4 | high | `test_repo_mode_preserves_native_records_and_gates` | build |
| Q6 | AC5 | high | `test_explicit_workflow_source_overrides_detection` | build |
| Q7 | AC5 | medium | `test_repo_without_workflow_keeps_defaults` | build |
| Q8 | AC6 | high | `test_domain_workflow_is_not_a_development_pipeline` | build |
| Q9 | AC6 | high | `test_conflicting_or_missing_sources_are_actionable` | build |
| Q10 | AC7 | high | `test_worktree_uses_its_own_workflow_documents` | build |
| Q11 | AC7 | high | `test_existing_ramify_order_survives_repo_workflow_selection` | build |
| Q12 | AC8 | high | Run workflow smoke cases, all Codex tests, and lifecycle tests | qa |
| Q13 | AC1–AC8 | medium | Inspect CLI help, setup skill, workflow/resume guidance, and README against the implementation | audit |
| Q14 | AC1–AC4 | medium | On a temporary Moship-shaped repo, run setup, start a story worktree, and inspect a fresh-session hook response | qa |

## Write set

```text
bin/ramify
libexec/setup.sh
libexec/workflow.py
libexec/repo-workflow*.py
libexec/statusline.sh
hooks/*
skills/*/SKILL.md
test/*
README.md
examples/moship.conf
.ramify/work/repo-workflows/*
```

Keep existing work orders and Moship's checkout unchanged.
