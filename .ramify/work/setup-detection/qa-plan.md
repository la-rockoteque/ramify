# QA plan: setup-detection

Every case is a check in the "setup drafts a config" section of `test/smoke.sh`. Each one runs `ramify setup --print` on a throwaway fixture repo. The fixtures need no network and no install. They use a fixture registry through `RAMIFY_REGISTRY`, so the user's registry never leaks in.

| Case | AC | Risk | Check | Settled by |
|---|---|---|---|---|
| Q1 | AC1 | high | `setup installs a bun repo with bun and waits for a file bun writes` | build |
| Q2 | AC1 | high | `setup runs a bun dev server with bun` | build |
| Q3 | AC2 | medium | `setup installs a yarn repo with a frozen lockfile` | build |
| Q4 | AC3 | medium | `setup installs an npm lockfile with npm ci` | build |
| Q5 | AC4 | medium | `setup syncs a uv repo and waits for its venv` | build |
| Q6 | AC5 | medium | `setup takes packageManager over the lockfile` | build |
| Q7 | AC6 | high | `setup installs a workspace once at its root, without cd` | build |
| Q8 | AC7 | medium | `setup runs react-scripts with BROWSER=none and PORT` | build |
| Q9 | AC8 | high | `setup passes PORT in the environment to a plain dev script` | build |
| Q10 | AC9 | medium | `setup leaves out a NestJS backend with a fixed port and says why` | build |
| Q11 | AC10 | high | `setup moves a base port off a range the registry holds` | build |
| Q12 | AC10 | high | `setup never gives two services the same base port` | build |
| Q13 | AC11 | high | `setup copies an ignored .env that .env.example implies` | build |
| Q14 | AC11 | medium | `setup copies ignored .env.* files that exist` | build |
| Q15 | AC11 | medium | `setup does not copy .env.example` | build |
| Q16 | AC12 | medium | `setup finds a compose file two levels down` | build |
| Q17 | AC12 | medium | `setup writes no compose file without containers` | build |
| Q18 | AC13 | medium | `setup skips ticket-key branch prefixes` | build |
| Q19 | AC14 | medium | `setup drafts a mobile app as branching only with QA notes` | build |
| Q20 | AC15 | low | `setup says it found no dev server, not a library` | build |
| Q21 | AC15 | low | `setup marks the QA examples as examples` | build |
| Q22 | AC16 | high | the existing setup checks and `every script parses under /bin/bash` pass in `bash test/smoke.sh` | build |
| Q23 | AC16 | medium | the whole suite runs: `bash test/smoke.sh`, each `test/*.py` with python3, each `test/*.js` with node. `test/codex.py` `test_resume_preserves_unfinished_build` fails on main too, so it does not count. | build |

No case runs on the live stack. The change only drafts text, and the fixtures cover each rule.

## Write set

May change:

- `libexec/setup.sh`
- `test/smoke.sh`
- `skills/setup/SKILL.md`
- `.ramify/work/setup-detection/**`

Must not change:

- `bin/**`
- `hooks/**`
- every other `libexec/**` file
