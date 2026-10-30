# Review: setup-detection

A separate reviewer agent read the branch diff against `spec.md` and `qa-plan.md`. It ran throwaway fixtures through `libexec/setup.sh --print`. This file lists each finding and its fix. All fixes are in `libexec/setup.sh` and `test/smoke.sh`.

## Findings and fixes

| # | Severity | Where | Failure | Fix | Check |
|---|---|---|---|---|---|
| 1 | high | `setup.sh` python, `os.listdir` | An unreadable directory, such as a Docker volume with mode 000, stopped the python pass. The draft had no install and no service, and showed no error. | Skip a directory that cannot be read. Print a warning when the python pass fails. | `setup survives a malformed package.json and an unreadable directory` |
| 2 | high | `setup.sh` python, `deps`, `load` | `"dependencies": null`, a non-object `package.json` or a non-string script stopped the python pass. | Read only object tables and string scripts. | same check |
| 3 | medium | `setup.sh` output | A quote in a path broke the sourced config, so every ramify command failed. `_dir` and `RAMIFY_COMPOSE_FILE` were not quoted. | Quote `_dir`, `_cmd`, `RAMIFY_QA_NOTES` and `RAMIFY_COMPOSE_FILE`, and escape single quotes. | `a draft with a quote in a path still loads` |
| 4 | medium | `setup.sh` READY | READY took the first install's marker. The installs run in turn, so `up` could start before the last install ended. | READY takes the last marker. | `setup skips an install with nothing to install and waits for the last one` |
| 5 | medium | `setup.sh` python | A root `package.json` with no dependency and no lockfile got `npm install`. npm writes no marker then, so the hook would wait for nothing. | Skip an install root with no lockfile, no workspace and no dependency. | same check |
| 6 | medium | `setup.sh` python | A package with its own lockfile and no dev server, such as a fixed-port NestJS app, got no install. | Every package adds its install root. | read in the diff |
| 7 | medium | `setup.sh` `manager` | A `pnpm-workspace.yaml` without a lockfile got npm. | Return pnpm for it. | `setup installs a pnpm workspace with pnpm` |
| 8 | medium | `setup.sh` python | A root `turbo run dev` script became a service and started every package a second time. | Skip a script that runs turbo, nx, lerna, concurrently, `--workspaces`, `--filter` or `-r`. Write a comment that names it. | `setup leaves out a root script that runs the other packages` |
| 9 | low | `setup.sh` bun READY | A bun workspace root with no dependency of its own got an empty READY. | Take the first dependency of any package under that root. | `setup waits for a dependency bun installs when no dev server bin is known` |
| 10 | low | `setup.sh` script match | `\b` matched `vite-node` as vite and `scripts/next-step.js` as next. | Match the command word only. | `setup does not take vite-node for vite` |
| 11 | low | `setup.sh` registry | A registry line with a trailing slash or a symlink did not match the repo, so a re-draft moved off its own ports. | Compare real paths. | read in the diff |
| 12 | low | `setup.sh` registry | An indented or `export`ed port was not held. | Drop the indent and `export` before the match. | read in the diff |
| 13 | low | `setup.sh` mobile | Only the first mobile app got notes. | Join the notes of every app. | read in the diff |
| 14 | low | `setup.sh` copy | A mobile app or a fixed-port app did not get its `.env` scanned. | Scan every package directory. | read in the diff |
| 15 | low | `setup.sh` ASP.NET | The `VITE_API_TARGET` prefix matched only `<pm> run dev`. | Add the prefix to every service command except Storybook. | `setup moves the API, its shared port and Storybook off held ports` |
| 16 | low | `test/smoke.sh` | The `.env.example` check passed without the code it guards, because the fixture un-ignored the file. | Check an ignored, untracked `.env.sample` instead. | `setup does not copy an ignored .env.sample` |
| 17 | low | `test/smoke.sh` | pnpm, npm without a lockfile, the bun fallback and the ASP.NET ports had no check. | Add a check for each. | the checks above |
| 18 | low | `setup.sh` python | Redundant `list()` and `set()`, and two lists built from one value. | Remove them. | read in the diff |

Findings 6, 8, 9, 13 and 14 go a little past the spec text. Each one makes the draft closer to what the spec asks for. Finding 8 covers a case the spec did not name.

## Spec change after approval

The coordinator reported a Rust repo (`Cargo.toml` and `Cargo.lock`) with an empty `RAMIFY_BOOTSTRAP`. The spec now lists it as a follow-up under "Out of scope". The branch does not change it.

## Checks after the fixes

- `bash test/smoke.sh`: all passed, every script parses under `/bin/bash` 3.2.
- `test/*.py`: all pass except `test/codex.py` `test_resume_preserves_unfinished_build`, which fails on main too.
- `test/*.js`: all pass.
- Two smoke checks fail at random on main and on this branch: "cleanup kills the stray's children" and "down stops the web's grandchildren". They passed in the last run. This branch does not touch that code.
