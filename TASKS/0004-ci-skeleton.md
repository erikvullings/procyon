# 0004 CI skeleton

Status: done
Priority: medium
Owner: unassigned
Agent: claude
Area: tooling
Depends on: 0003

## Context
`file-manager-coding-agent-spec.md` §31 lists the required CI jobs. Create the skeleton early so
every later task lands green; jobs for features that do not exist yet are added by their own task.

## Acceptance Criteria
- GitHub Actions workflow(s) running on push and pull request with jobs for:
  Rust fmt, Clippy with `-D warnings`, `cargo test --workspace`, frontend format check,
  `tsc --noEmit`, Vitest, frontend production build.
- Cargo registry/target and pnpm store caching configured.
- Dependency audit job (`cargo audit` / `pnpm audit`) that does not block on advisories without a
  fix, but reports them.
- Matrix runs on ubuntu-latest, macos-latest and windows-latest for the Rust jobs.
- No code signing in PR builds (§31); release signing is out of scope until 0063.
- A README badge or short CI section documents what runs.

## Implementation Notes
- `api:check` (0010) and the Tauri macOS/Windows build jobs (0015) are added by those tasks.
- Keep workflow files small and composable, use the latest version; prefer a reusable job over copy-paste.

## Agent Notes
- 2026-07-29 claude: Added `.github/workflows/ci.yml` with three jobs. `rust`
  (matrix: ubuntu-latest, macos-latest, windows-latest) runs `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`, caching
  the registry/target via `Swatinem/rust-cache@v2` (which auto-installs the pinned
  `rust-toolchain.toml` toolchain, so no separate toolchain-install step was needed). `frontend`
  (ubuntu-latest only, no matrix — not required by the AC and would be wasted spend) runs
  `pnpm run lint:frontend` (Biome format/lint), `tsc --noEmit` via the frontend's `typecheck`
  script, `pnpm run test:frontend` (Vitest) and `pnpm run build:frontend` (production build),
  caching the pnpm store via `actions/setup-node`'s built-in `cache: pnpm`. `audit` (ubuntu-latest)
  installs `cargo-audit` via `taiki-e/install-action` and runs `cargo audit` and
  `pnpm audit --audit-level moderate`; the whole job is `continue-on-error: true` (and each audit
  step individually too) so advisories never block push/PR checks, while still printing full
  findings in the job log. Triggers on `push` to `main` and all `pull_request`s; added a
  `concurrency` group to cancel superseded runs (standard, low-risk addition, not itself an AC
  item). No code-signing/notarization step exists anywhere in the workflow, satisfying "no code
  signing in PR builds" by omission — that's added later by task 0063.
- 2026-07-29 claude: Test-driven: added `scripts/ci-workflow.test.mjs` (new root devDependency
  `js-yaml`, used only by this test) which parses the workflow YAML for real — not regex-matching
  — and asserts: push/pull_request triggers exist; the `rust` job matrixes over exactly the three
  required OSes and uses `${{ matrix.os }}` for `runs-on`; the `rust` job's steps contain the exact
  fmt/clippy/test commands; a `rust-cache` action is present; the `frontend` job runs on
  ubuntu-latest with no `strategy` (i.e. no matrix); its steps cover format check, typecheck, test
  and build; `actions/setup-node` is configured with `cache: pnpm`; an `audit` job exists, runs
  both `cargo audit` and `pnpm audit`, and is non-blocking (job- or step-level
  `continue-on-error: true`); no `codesign`/`notariz` strings appear anywhere; and `README.md`
  contains a CI badge and a `## CI` section. Written test-first — confirmed red (`ENOENT`, no
  workflow file) before writing `ci.yml`, then green. Also caught two real issues before they could
  reach CI: (1) js-yaml v5's ESM build has no default export (`import { load } from 'js-yaml'`,
  not `import yaml from 'js-yaml'`); (2) my first `on.pull_request` assertion used `assert.ok(...)`
  against a YAML key with no value (`pull_request:` parses to `null`), which is falsy despite being
  valid/present — fixed to check key presence (`'pull_request' in workflow.on`) instead.
- 2026-07-29 claude: Added a root `README.md` (didn't exist before this task) with a CI badge
  linking to the workflow, a one-line project/spec/TASKS pointer, and the short "## CI" section the
  AC requires, describing each job. No `actionlint`/`yamllint` binary was available locally to lint
  the workflow's GitHub Actions-specific semantics (e.g. invalid `uses:` refs) — the YAML-parsing
  test suite above catches structural regressions but not that class of error; flagged here as a
  known gap rather than silently skipped. `cargo-audit`/`pnpm audit` were not run locally (no
  network in the dev sandbox for `cargo install cargo-audit`); their behavior is only exercised
  once the workflow runs on GitHub.
- 2026-07-29 claude: Verified — `pnpm run test:scripts` passes all 14 tests (10 new in
  `ci-workflow.test.mjs`, 4 pre-existing from task 0003, unchanged). `pnpm run test` (full suite:
  rust workspace, frontend Vitest, scripts) passes. `pnpm run lint` passes (`cargo fmt --all
  --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean; `biome check .` exits 0
  with only the same pre-existing info-level `vite.config.ts` suggestion noted in task 0003's Agent
  Notes — a new `noTemplateCurlyInString` warning on the test file's literal `${{ matrix.os }}`
  string was suppressed with a `biome-ignore` comment since it's GitHub Actions expression syntax,
  not a JS template literal). `pnpm run build:frontend` succeeds.
