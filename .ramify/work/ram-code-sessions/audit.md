# Audit: Ram Code sessions

| AC | Verdict | Evidence |
|---|---|---|
| AC1 | met | `sessions()` records the primary's basename as the project and sorts by it; `visible_rows` emits one header per project with its count and only the open group's sessions. The renderer draws them. Test `sessions_group_by_project`. |
| AC2 | met | `Enter` and `Space` on a header set `open_group`, which every group's visibility reads. Test `opening_a_group_closes_the_others`. |
| AC3 | met | `Enter` on a session sets `pending_open`; `App::on_key` opens that root and reloads. Test `enter_on_a_session_opens_its_root`. |
| AC4 | met | `stop_agents` runs `down` with the selected worktree root and stores the output. Test `agent_stop_runs_down`. |
| AC5 | met | The other panes keep their empty states and keys. Test `other_panes_keep_working`; the existing panel tests also pass. |

Write set: the diff stays inside it.

## Notes

- The panel keeps the worktree it was given when the app has no selection, so the Agents pane
  works outside a loaded project.
- The Electron windows are untouched.
