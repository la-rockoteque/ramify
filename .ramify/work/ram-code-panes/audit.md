# Audit: Ram Code panes

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `Panel::sessions` reads `$STATE_DIR/sessions/*.root` and `.name`; `git_branch` adds the branch; `lines` shows name, branch and root. Test `sessions_list_names_roots_branches`. |
| AC2 | met | `session_color` uses the MD5 palette; `border_color` returns it in the Sessions pane. Test `selected_session_colour_themes_panel`. |
| AC3 | met | `Enter` in Sessions sets `pending_open`; `App::on_key` calls `open_root`, which resets the conversation and reloads. Test `enter_opens_session_root`. |
| AC4 | met | `agent_entries` reads the worktree's pid files from the state directories; the empty state names `ramify up`. Test `agents_list_the_worktree_services`. |
| AC5 | met | `artifact_entries` finds the order by branch and lists the documents; `lines` previews the selected one. Test `artifacts_list_and_preview`. |
| AC6 | met | The four existing panes keep their empty states and keys. Test `existing_panes_still_work`. |

Write set: the diff stays inside it.

## Notes

- Sessions and artifacts read state directly; no CLI call is needed for them.
- The Electron windows are untouched and still serve Claude Code and Codex.
