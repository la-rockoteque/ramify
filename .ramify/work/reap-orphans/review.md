# Review: reap orphans

## Checked, nothing to change

- reap reaches processes only through `kill_worktree`, behind the tag guard of `down`. It never signals an untagged process.
- It stops only `orphan` stacks. `zombie`, `merged` and `pr-open` stay: a person may still use them.
- The hook returns at once. The stamp is written before reap starts, so two sessions starting together start one reap.
- A state dir that cannot be written stops the hook call silently. It never fails a session start.
- The unreadable-worktree-list guard covers `mem` and `mem --apply` too. That closes the 0.10.0 defect named in the spec.
- Every script parses under `/bin/bash` 3.2.

## Known limit

- A stack whose last live member is a platform binary, such as `/bin/sleep`, shows no tag. reap leaves it running and prints "left alone", as `mem --apply` does.
- Sessions started before the update run the old hook. They do not reap until they reload.

## Results

`test/smoke.sh`: all passed, 10 new checks included. `test/codex.py` fails as on `main`.
