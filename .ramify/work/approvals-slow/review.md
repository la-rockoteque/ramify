# Review: approvals-slow

## Fixed during review

- `gates` dropped an index entry when the check of its worktree failed, for example on a timeout under load. The request then stayed hidden until the next full scan, up to 30 minutes, while an agent waited on it. An entry now goes only when the check succeeds and finds no request, or when the worktree is gone. `gate_check` follows the same rule.

## Checked, nothing to change

- Every request change goes through `write_order`: ask, ensure, approve and reject. Each one updates the index.
- A read-only resume writes no work order, so it creates no index entry. `test/codex.py` is unchanged: its one failure is the same as on `main`.
- A worktree without a work order answers `null` with exit 0, so its entry goes.
- Python names an entry by the hash of `realpath`, and bash by the hash of `pwd -P`. Both give the same real path.
- The new launch folder is under `~/.cache/ramify/electron/launch/`. The app writes its logs there already.

## Known limit: the switch to the new version

Sessions that started before the update run the old hooks and the old launcher.

- The old launcher still quits every window of another build, the newer one too. The window can restart until those sessions end or run `/reload-plugins`.
- The old `workflow.py` writes no index entry. Such a request shows up at the next full scan, within 30 minutes. A new `gates` after a reboot scans at once.

## Results

- `test/smoke.sh`: all passed, the new index checks included.
- `test/app_launch.py`: 8 tests pass, the 2 new ones included.
- On this machine: `gates` took 98 s before. Now the first call (full scan) takes 46 s and later calls take 2 s.
