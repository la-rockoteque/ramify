# Repository workflow QA results

| Case | Result | Evidence |
|---|---|---|
| Q1 | pass | `test_setup_detects_moship_pipeline_documents`. A read-only setup draft against Moship's actual worktree selects all five pipeline guides. |
| Q2 | pass | `test_setup_print_reports_sources_without_writes` and `test_setup_print_with_existing_config_creates_no_runtime_state`. |
| Q3 | pass | `test_repo_workflow_routes_cli_and_session_guidance` checks CLI output and Claude/Codex session responses. |
| Q4 | pass | `test_new_uses_repo_workflow_without_duplicate_order` creates two real worktrees, with and without `--workflow`, and preserves the configured branch prefix. |
| Q5 | pass | `test_repo_mode_preserves_native_records_and_gates` preserves the native file byte-for-byte and rejects default record mutations. |
| Q6 | pass | `test_explicit_workflow_source_overrides_detection` checks explicit Ramify selection and structured workflow disablement. |
| Q7 | pass | `test_repo_without_workflow_keeps_defaults`. |
| Q8 | pass | `test_domain_workflow_is_not_a_development_pipeline` covers incidental development, review, and equipment testing language. A separate regression excludes linked CI YAML from documentation discovery. |
| Q9 | pass | `test_conflicting_or_missing_sources_are_actionable`. |
| Q10 | pass | `test_worktree_uses_its_own_workflow_documents` and `test_setup_from_worktree_reads_its_current_documents`. |
| Q11 | pass | `test_existing_ramify_order_survives_repo_workflow_selection`. |
| Q12 | pass | All 6 Codex tests and 21 Jira lifecycle tests pass. Smoke passes 187 cases with the two documented baseline colour failures. |
| Q13 | pass | README, setup/workflow/resume/worktree skills, configuration output, statusline, and Moship example describe repository routing and precedence. |
| Q14 | pass | The worktree and session tests run the real CLI and hooks against temporary Moship-shaped repositories. |

Yield: 15 repository workflow tests, 6 Codex tests, and 21 Jira lifecycle tests pass. Smoke passes 187 cases; two existing terminal-colour cases fail.

## Acceptance audit

AC1–AC2: `repo-workflows.py` detects development guides and emits a quoted configuration. Setup reports the reason and source paths without writing in print mode.
AC3: `workflow.py` routes repository mode to native guidance; session hooks and the statusline avoid the default step instructions.
AC4: Native start prints the guide paths without writing an order. Native mutation commands explain how to use the repository's own records.
AC5: Explicit structured definitions and source choices precede discovery; absent evidence keeps the defaults.
AC6: Discovery requires development-process evidence. Conflicting or missing sources produce actionable errors.
AC7: Sources resolve within the current checkout. A matching existing Ramify order retains its original record and commands.
AC8: Existing workflow and integration suites run against the branch.

The separate reviewer found two issues: an old order could affect a new native branch name, and product documents could trigger detection.
Branch selection now uses the new task's context. Discovery requires an explicit software-process heading or positive code-delivery evidence.
Both regressions pass. The reviewer confirmed the fixes and found no remaining actionable issues.

## Limits

Ramify routes to the repository's workflow; it does not translate arbitrary prose into executable steps.
Native repository hooks enforce native gates. Ramify does not invent a second checklist.
Moship was a read-only reference. Its files and configuration were not changed.
