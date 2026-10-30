# Diagnose: the approvals window is slow and restarts

## Symptom

The user reports it: when several sessions ask for approval, the approvals window is very slow to load, and the app restarts each time.

## Evidence

- `ramify gates --json` took 98 s on this machine, with 4 open requests.
  The window calls it with a 60 s timeout.
  `approvals.log` shows `Error occurred in handler for 'gates': Error: ramify gates exited 1`.
- The registry has 47 lines and 35 live projects. One listing makes 56 `ramify step asked --json` calls (bash and python each), plus one config load per project. One call takes 0.4 s at rest and more under load.
- `approvals.log` shows a new main process every 12 to 35 s: 31389, 36807, 44406, 47554. Each old one ends with exit code 15 (SIGTERM).
- `approvals.log` shows `EINVAL: invalid argument, open '/var/folders/…/T/ramify-launch-xe2v7enf/ready.json'`.
- 8 approval builds exist in `~/.cache/ramify/electron/apps/approvals/`. Sessions started before the update to 0.10.0 still run the 0.9.1 hooks.
- A launch from an unsandboxed shell, with one build, takes 0.8 to 1.4 s. Hashing the source takes 0.06 s. Those parts are not slow.

## Root causes

1. **`gates` reads every worktree of every project.** A request lives in the front matter of a work order. `gates_json` finds requests by running `step asked --json` in each worktree of each registered project. Its cost grows with projects and worktrees, not with open requests. 20 projects were added today.
2. **Two builds quit each other.** `retire_other_builds` sends SIGTERM to every window of another build. The single-instance lock lives in a profile dir per build, so two builds never hand over. A 0.9.1 session and a 0.10.0 session that ask in turn each quit the other's window.
3. **The ready file is in the caller's `$TMPDIR`.** A sandboxed session has its own `$TMPDIR`. The app cannot open a file there (EINVAL). The launcher then waits for the full 30 s timeout and reports a failure.

## Failing tests

- `test/smoke.sh`, new section "gates reads an index":
  - `step ask records the request in the gate index`
  - `gates drops an index entry whose request is gone`
  - `gates finds a request with no index entry after the index is lost`
- `test/app_launch.py`:
  - `test_newer_window_survives_an_older_launch`
  - `test_ready_file_is_under_the_cache`

## Fix plan

1. `step ask` and the hook's `ensure` write an index entry, `/tmp/ramify/gates/<hash of the worktree path>`, with the worktree path.
   `gates` reads only the index. It checks each entry with one `step asked --json` and removes an entry whose request is gone.
   When the index dir is missing (a reboot, or the first run after the update), `gates` scans every worktree once, as now, and fills the index.
   A scan older than 10 minutes starts a new full scan in the background, so a request from an older ramify is found within 10 minutes.
2. `retire_other_builds` quits only windows of builds older than this one, by bundle mtime. When a newer build's window is open, the launch hands over to it.
3. The launcher makes its ready folder under the cache (`~/.cache/ramify/electron/launch/`). Both the caller and the app can write there.

## Lane

Bug, hotfix lane. The index changes how `gates` finds requests, so the review should check that no request can be lost.
