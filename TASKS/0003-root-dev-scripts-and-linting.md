# 0003 Root development scripts, formatting and linting

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: tooling
Depends on: 0001, 0002

## Context
`file-manager-coding-agent-spec.md` §32 lists the root-level commands the project must provide,
and §2.3 lists the API pipeline scripts. Shared linting/formatting is part of §33 step 1.

## Acceptance Criteria
- Root `package.json` provides the scripts from §32: `dev`, `dev:mock`, `dev:http`, `dev:tauri`,
  `test`, `test:rust`, `test:frontend`, `lint`, `api:export`, `api:generate`, `api:check`,
  `build`, `build:tauri`.
- Scripts that cannot work yet (e.g. `dev:tauri`, `api:generate`) either fail with a clear
  "not implemented until task NNNN" message or are added by the task that enables them — no silent
  no-ops.
- `scripts/export-openapi.sh` and `scripts/generate-api.sh` exist and are executable.
- Rust: `rustfmt.toml` + `cargo fmt --check` and `cargo clippy -- -D warnings` wired into `lint`.
- Frontend: a formatter and linter configured (prettier + eslint, or biome) with a single
  `pnpm lint` entry point; formatting settings shared across the repo where possible.
- `.editorconfig`, `.gitignore` (target/, node_modules/, dist/, generated openapi artefacts kept in
  git per §2.3) and `.gitattributes` for deterministic line endings on Windows.
- `AGENTS.md` at the repo root summarising the coding-agent rules from §35.

## Implementation Notes
- Root scripts orchestrate pnpm workspaces; the frontend keeps its own scripts.
- Keep generated code clearly separated and never hand-edited (§35).

## Agent Notes
- 2026-07-29 claude: Implemented. Root is now a pnpm workspace (`pnpm-workspace.yaml` lists
  `frontend`) with a root `package.json` providing every §32 script plus `test:rust`,
  `test:frontend`, `lint:rust`, `lint:frontend`, `build:rust`, `build:frontend` as the pieces each
  composite script chains together. `dev`/`dev:http` run the frontend dev server directly (the
  README's recommended flow already runs `cargo watch` in a second terminal); `dev:mock` and
  `dev:http` set `VITE_RUNTIME` via `cross-env` for Windows compatibility.
- 2026-07-29 claude: `scripts/export-openapi.sh`, `scripts/generate-api.sh` and
  `scripts/not-implemented.sh` exist and are executable (`chmod +x`). Per the "no silent no-ops"
  criterion: `export-openapi.sh` runs `cargo run -p fm-server -- export-openapi` and fails loudly
  if the output still contains the crate's current placeholder text (fm-server doesn't parse args
  yet); `generate-api.sh` fails loudly if `frontend/orval.config.ts` doesn't exist yet;
  `dev:tauri`/`build:tauri` call the generic `not-implemented.sh <script> <task>` helper. All three
  point at the exact task that unblocks them (0009, 0010, 0015 respectively) and are covered by a
  new `node --test` suite, `scripts/scripts.test.mjs` (4 tests: executability, and that each of the
  three failure paths exits non-zero with the expected task number in stderr) — written test-first,
  which caught a wording mismatch between `generate-api.sh`'s message and the test's expectation on
  the first run.
- 2026-07-29 claude: Rust lint was already fully wired by task 0001 (`rustfmt.toml`, `clippy.toml`,
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`); this task
  only exposes it as the root `lint:rust` script.
- 2026-07-29 claude: Chose Biome over ESLint + Prettier for the frontend, added as a root
  devDependency with a single `biome.json` (2-space indent, single quotes, trailing commas,
  organize-imports, `vcs.useIgnoreFile` so it respects `.gitignore`). Reasons: one dependency
  instead of ~6, one config file, and it doesn't depend on `typescript-eslint`'s compatibility with
  the repo's TypeScript 7 (very new at time of writing). `pnpm run lint:frontend` is the single
  entry point (`biome check .`); running it against the existing 0002 code found three real
  organize-imports violations (fixed) and one purely-cosmetic `useLiteralKeys` suggestion on
  `vite.config.ts`'s `process.env['FM_SERVER_ORIGIN']`, deliberately left as-is since Biome marks
  its own fix "unsafe" and it doesn't fail the check (info-level, exit 0).
- 2026-07-29 claude: Added root `.editorconfig` (LF, 2-space default, 4-space for `*.rs`),
  `.gitattributes` (`* text=auto eol=lf`, binary rules for images/fonts, CRLF for `*.bat`/`*.cmd`),
  and extended `.gitignore` with tooling logs, editor/OS junk and `.env*` — deliberately did not
  touch anything under `frontend/openapi/` so generated OpenAPI artefacts stay tracked per §2.3.
  Root `AGENTS.md` summarises the §35 must/must-not lists and documents where the two generated
  artefacts (OpenAPI document, Orval client) will live and how they're regenerated.
- 2026-07-29 claude: Verified — `pnpm run test` (rust: `cargo test --workspace`, unchanged 9 tests
  from task 0001; frontend: 14 tests across 3 files, unchanged from task 0002; scripts: 4 new tests
  in `scripts/scripts.test.mjs`, all passing). `pnpm run lint` clean (`cargo fmt --all --check` +
  `cargo clippy --workspace --all-targets -- -D warnings` clean; `biome check .` exits 0). `pnpm run
  build` succeeds (`cargo build --workspace --release` and `tsc --noEmit && vite build`). Confirmed
  by hand that `api:export`, `api:generate`, `dev:tauri` and `build:tauri` each exit 1 with a
  message naming the blocking task, and that `api:check` short-circuits at `api:export` rather than
  reporting a stale-diff false pass.
- 2026-07-29 claude: Known gap — `dev` does not also start `cargo watch -x "run -p fm-server"`;
  the developer still runs that in a second terminal as the spec's "Recommended development flow"
  describes. Combining both into one `dev` script would need a new dependency (e.g. `concurrently`)
  that isn't required by any acceptance criterion, so it was left out rather than added
  speculatively.
