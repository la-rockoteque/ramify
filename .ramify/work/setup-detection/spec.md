# Spec: setup-detection

## Problem

`ramify setup` drafts a `.ramify.conf` from the repo. On 19 real repos the draft was wrong or empty most of the time, and every config needed a hand rewrite. A wrong `RAMIFY_READY` is the worst case: the worktree hook waits for a file that the installer never writes.

## Behaviour

`libexec/setup.sh` keeps its interface (`--print`, `--force`, `--local`) and its output layout. These parts of the detection change.

### Install and READY

1. The draft finds the package manager of each install root. `packageManager` in `package.json` wins. Else the lockfile decides:

   | Lockfile | Install | READY |
   |---|---|---|
   | `bun.lock`, `bun.lockb` | `bun install` | `node_modules/.bin/<dev server bin>`, else `node_modules/<first dependency>/package.json` |
   | `pnpm-lock.yaml` | `pnpm install` | `node_modules/.modules.yaml` |
   | `yarn.lock` | `yarn install --frozen-lockfile` | `node_modules/.yarn-integrity` |
   | `package-lock.json` | `npm ci` | `node_modules/.package-lock.json` |
   | none | `npm install` | `node_modules/.package-lock.json` |
   | `uv.lock` | `uv sync` | `.venv/pyvenv.cfg` |

2. An install root is the nearest directory, from the package up to the repo root, that has a lockfile, a `packageManager` field, a `workspaces` field or a `pnpm-workspace.yaml`. A package without one installs in its own directory. Each install root gets one install, so a workspace installs once at its root.
3. A root that has a `package.json` gets an install even when it has no dev server.
4. An install at the repo root has no `(cd "." && …)` wrapper.

### Services

5. For each `package.json` (root, one and two levels down, never in `node_modules`), the draft picks the first script of `dev`, `start:dev`, `start`, `serve`.
6. The script text decides the command. `<pm>` is the package manager of the install root.

   | Script runs | Command | Base port |
   |---|---|---|
   | `next` | `<pm> run <script> -p $PORT` | 3000 |
   | `vite`, `astro`, `webpack serve`, `webpack-dev-server` | `<pm> run <script> --port $PORT` | 5173 (webpack 8080) |
   | `react-scripts start` | `BROWSER=none PORT=$PORT <pm> run <script>` | 3000 |
   | anything else | `PORT=$PORT <pm> run <script>` | 3000 |

   npm adds `--` before the flags.
7. A NestJS package (`@nestjs/core`) whose `src/main.ts` calls `.listen(` with a number is not a service. The draft adds a comment that names it and says "port not configurable".
8. The first UI server (next, vite, astro, webpack, react-scripts) is `web`. Other services take their directory name, and `app` at the root.

### Ports

9. The draft reads every config in the registry (`$RAMIFY_REGISTRY`, else `~/.claude/ramify/projects`). It skips the repo it drafts. A `<svc>_port=P` holds `P` to `P+RAMIFY_MAX_SLOT` (that config's value, else 5). A `<svc>_shared_port=P` holds `P`.
10. Each drafted port starts at its default base. It moves up in steps of 10 until its range overlaps no held range and no earlier drafted port. This covers service ports, Storybook and the API shared port.

### Copy

11. `RAMIFY_COPY` holds a gitignored, untracked path that exists: `.claude`, `.envrc`, `.vscode/settings.json`, and every `.env` or `.env.*` file except `.env.example`, `.env.sample`, `.env.template`. The draft looks at the repo root and at each service directory.
12. When `.env.example` exists in a directory and git ignores `.env` there, the draft adds `.env`, even when `.env` is absent. The same rule applies to `.env.local`.

### Compose

13. The draft looks for a compose file at the root, then one level down, then two levels down (`packages/*/`).
14. When the compose file has no `container_name`, the draft writes both settings as comments with a note. It never writes `RAMIFY_COMPOSE_FILE` with an empty `RAMIFY_CONTAINERS`.

### Branch prefix

15. The prefix count skips all-caps ticket-key prefixes such as `IN-3/`.

### Mobile apps

16. A package with a `react-native` or `expo` dependency, or with `ios/` and `android/` next to it, is not a service. The draft keeps branching and the install. It sets `RAMIFY_QA_NOTES` to a stub that names the package's `ios`, `android`, `test` and `lint` scripts that exist.

### Comments

17. The `RAMIFY_QA_RESULTS` and `RAMIFY_QA_NOTES` examples say they are examples.
18. When nothing is detected, the comment says the draft found no dev server. It does not say the repo is a library or a plugin.

## Acceptance criteria

- **AC1**: Given a repo with `bun.lock` and a `vite` dev script, the draft has `RAMIFY_BOOTSTRAP='bun install'`, `RAMIFY_READY=node_modules/.bin/vite` and `web_cmd='bun run dev --port $PORT'`.
- **AC2**: Given `yarn.lock`, the draft installs with `yarn install --frozen-lockfile` and waits for `node_modules/.yarn-integrity`.
- **AC3**: Given `package-lock.json`, the draft installs with `npm ci` and waits for `node_modules/.package-lock.json`.
- **AC4**: Given `uv.lock` at the root, the draft runs `uv sync` and waits for `.venv/pyvenv.cfg`.
- **AC5**: Given `packageManager: "bun@1.1.0"` and a `package-lock.json`, the draft uses bun.
- **AC6**: Given a root `package.json` with `workspaces` and two packages with dev scripts, the bootstrap is one install at the root, with no `cd`.
- **AC7**: Given a `react-scripts start` script, the command is `BROWSER=none PORT=$PORT <pm> run start`.
- **AC8**: Given a `dev` script that runs `bun run src/index.ts`, the command is `PORT=$PORT bun run dev`.
- **AC9**: Given a vite UI and a NestJS backend with `.listen(3000)`, the services are `web` only, and a comment names the backend with "port not configurable".
- **AC10**: Given a registry config that holds 5173 to 5178, the drafted `web_port` is not in that range. Given two vite packages, their base ports differ.
- **AC11**: Given `.env.example` and a `.gitignore` that ignores `.env`, `RAMIFY_COPY` has `.env` although `.env` is absent. Given ignored `.env.development` and `.env.test` files, `RAMIFY_COPY` has both.
- **AC12**: Given `packages/db/docker-compose.yml` with a `container_name`, the draft has its path and the container. Given a compose file without `container_name`, the draft has no uncommented `RAMIFY_COMPOSE_FILE`.
- **AC13**: Given branches `IN-1/a`, `IN-2/b`, `IN-3/c` and `feat/x`, the prefix is `feat/`.
- **AC14**: Given an `expo` package with `ios` and `test` scripts, `RAMIFY_SERVICES` is empty and `RAMIFY_QA_NOTES` names both scripts.
- **AC15**: A repo with nothing to serve gets a comment without "library or plugin". The QA examples are marked as examples.
- **AC16**: The existing setup checks in `test/smoke.sh` still pass, and every script parses under `/bin/bash` 3.2.

## Out of scope

- Branch prefix weighted by merge commits.
- A branch convention read from `CLAUDE.md` or `CONTRIBUTING`.
- `RAMIFY_COMPOSE_SERVICES`, to start a subset of the compose services.
- `ramify config` that prints the port range and command of each service.
- Yarn Berry and its `.yarn/install-state.gz`.
- Python dev servers. `uv.lock` gets an install, not a service.
- Follow-up, found after approval: a Rust repo (`Cargo.toml` and `Cargo.lock`) should draft `RAMIFY_BOOTSTRAP='cargo fetch --locked'` with an empty `RAMIFY_READY`, because cargo writes no marker. It should also draft a `RAMIFY_QA_NOTES` stub with `cargo test` and `cargo clippy --all-targets`.

## Open questions

None.

## Lane

Light. The change stays in the draft that a human reviews. It touches no invariant and no external contract, and `setup.sh` already has tests in `test/smoke.sh`.
