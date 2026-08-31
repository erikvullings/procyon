# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Environment Setup
- Local macOS capabilities and optimized CLI tools are mapped in `~/.config/ai/tools.md`. Read this file to use optimized search/replace and parsing binaries.

## What this is

Procyon: a dual-pane file manager. Rust workspace (Axum server + Tauri desktop shell) with a
Mithril/TypeScript frontend shared by both hosts. Full spec:
[file-manager-coding-agent-spec.md](file-manager-coding-agent-spec.md) (authoritative wherever this
file or `AGENTS.md` drift from it). Task-by-task implementation status:
[TASKS/README.md](TASKS/README.md) — read the relevant `TASKS/NNNN-*.md` file before touching an
area it covers; it establishes the contract (types, module boundaries), not just history.

## Commands

Run from the repo root via `pnpm run <script>`:

| Command | What it does |
| --- | --- |
| `pnpm dev` | Vite dev server against the in-process **mock** client (default, no Rust process needed) |
| `pnpm dev:http` | Vite dev server against the **Axum backend** (`VITE_RUNTIME=http`) — pair with `pnpm dev:server` |
| `pnpm dev:server` | `fm-server` on port 8787, auth disabled, auto-rebuild on change |
| `pnpm dev:tauri` | Tauri desktop app in dev mode |
| `pnpm test` | Rust + frontend + script tests (see below to run a subset) |
| `pnpm lint` | `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` + Biome |
| `pnpm api:export` / `api:generate` | Regenerate `frontend/openapi/openapi.json` / the Orval client — never hand-edit either |
| `pnpm api:check` | Fails if either generated file is stale relative to the backend |
| `pnpm build` / `build:tauri` | Production build / packaged desktop app |

Running a single test:

```bash
cargo nextest run -p fm-application requires_confirmation_then_deletes   # one Rust test by name
cargo test -p fm-application --lib thumbnail::                          # a module's tests
pnpm --dir frontend exec vitest run src/features/panes/pane.test.ts     # one frontend test file
pnpm --dir frontend exec vitest run -t "lands the cursor on the first entry"
```

`cargo nextest run --workspace` is the full Rust suite (`cargo test --doc --workspace` separately
covers doctests, which nextest doesn't run).

**Rust toolchain:** pinned to `1.97.1` via `rust-toolchain.toml`, matching CI exactly — don't chase
build/lint differences without first confirming `rustc --version` matches. A Homebrew-installed
`rustc`/`cargo` ahead of `~/.cargo/bin` (rustup's shims) on `PATH` silently shadows this pin; a
newer local toolchain can then fail clippy lints (e.g. `result_large_err`) that don't exist in the
pinned version and that CI — which uses the correct pin — passes clean on identical code. Fix the
`PATH`, don't "fix" the flagged code. If a shell session still resolves the wrong `cargo`, prefix
one-off commands with `PATH="$HOME/.cargo/bin:$PATH"` rather than chasing the lint.

**Git hooks and their real cost:** `pre-commit` normally lints only the files staged in that commit
(rustfmt + clippy on the *owning crate* + Biome) and is fast. But `scripts/pre-commit.mjs` resolves
a staged file's "owning crate" by walking up to the nearest `Cargo.toml`, and the workspace root
`Cargo.toml` is its own manifest — so any commit that stages the **root** `Cargo.toml` (e.g. a
`[workspace.package].version` bump, which touches every crate's compiled metadata) triggers
`cargo clippy --workspace --all-targets`, a full-workspace build including `apps/fm-desktop/
src-tauri` (Tauri/wry/tao — the heaviest dependency subtree here). `pre-push` *always* runs that
same workspace-wide lint (but not the test suite — CI covers that) regardless of what's staged. On
a cold or invalidated `target/` cache either can take 20–90+ minutes. Always run `git commit`/
`git push` here backgrounded with a long timeout, never in the default ~10-minute foreground cap —
a foreground timeout kills the process and forces a retry from scratch. If two such commands run
back-to-back they'll queue behind each other for cargo's target-dir lock; that's normal, not a hang.
Use the authenticated `gh` CLI for GitHub operations; do not assume the raw `git` remote has valid
credentials. Run `gh auth setup-git` before a push when Git is not already using `gh` authentication.
If `GH_TOKEN` selects an account without write access while the authorized account is stored in the
`gh` keyring, unsetting the variable alone may not work because Copilot CLI injects its own credential
helper. Override both for that push:

```bash
env -u GH_TOKEN git \
  -c credential.helper= \
  -c credential.helper='!gh auth git-credential' \
  push -u origin "$(git branch --show-current)"
```

The empty `credential.helper` resets inherited helpers; the following helper delegates authentication
to the authorized keyring-backed `gh` account.

## Architecture

### Layering (spec §3, mechanically enforced)

```
Mithril frontend (components, state, panes, dialogs)
        │  depends only on FileManagerClient — never fetch/EventSource/Tauri APIs directly
        ├── HTTP client adapter (generated REST client + SSE)
        └── Tauri client adapter (invoke commands + channels/events)
                │
        Rust application services (fm-application: navigation, workspaces, actions,
        operations, search, metadata, plugins, settings, event publication)
                │
        Rust domain and engine (VFS providers, operation scheduler, conflict
        resolution, directory snapshots, watching, journaling)
```

Ten mandatory rules govern this boundary (spec §3, restated in
[docs/architecture/overview.md](docs/architecture/overview.md)); the two mechanically checked ones
— core engine crates must not depend on Axum/Tauri, and the `thiserror`-in-libraries/`anyhow`-in-
`apps/*` split — are asserted by `crates/fm-test-support/src/architecture.rs` against the real
`cargo metadata` graph (`workspace_architecture.rs` test). If prose and that code disagree, the
code is correct. The rest (Axum handlers and Tauri commands stay thin; DTOs aren't reused as domain
models; long-running work is a job, not a blocking call; the backend owns authoritative state;
browser and Tauri hosts must behave equivalently) are reviewed by hand at the point handlers/
commands are added — `fm-application` is the one real implementation both hosts call into.

### Application capability services

`FileManagerService` is the host-facing composition root, not the implementation home for new
backend features. Keep its public methods as thin delegation calls and put coordinating logic in a
deep capability module under `crates/fm-application/src/`. Existing seams include
`OperationsCoordinator` (scheduler, history, idempotency and conflict control),
`SearchComparisonCoordinator` (search/comparison lifecycle and sync plans), `ActionInvoker`
(core/plugin/platform action dispatch), `ChecksumCoordinator`, `FileEditorService`,
`ConnectionFacade`, and `PluginManager`. Extend the owning service when a feature fits one of these
capabilities; add a new service only for a genuinely distinct responsibility. Do not move logic
back into the facade or let capability services depend on `FileManagerService`.

### Runtime adapters (frontend)

`VITE_RUNTIME` picks the client adapter at build time: `mock` (in-process fixtures, no backend),
`http` (Axum + SSE), `tauri` (IPC + channels). All three implement the same `FileManagerClient`
interface consumed by every component — a feature that only works through one adapter is a bug
(spec rule 9). `fm-server` binds loopback-only by default and is API-only (no static file serving);
see [README.md § Running fm-server on a remote host](README.md#running-fm-server-on-a-remote-host)
for the reverse-proxy deployment pattern if that's relevant.

### Frontend state

Meiosis-style, not a general-purpose state library (see
[ADR 0007](docs/decisions/0007-frontend-state-management.md)): one state tree, actions that return
patches, `m.stream` composition. Shared state (workspaces, panes, selection, settings, connection
status) lives in the tree; components must not hold shared state locally. Don't introduce Redux/
MobX/Pinia-style stores or a second state mechanism — express new needs as new tree slices/actions.

### VFS and operations

Filesystem access goes through the `fm-vfs` provider trait (`fm-vfs-local`, `-sftp`, `-ftp`,
`-webdav`, plus the archive provider in `fm-archive`) — new location types are new providers, not
special-cased branches elsewhere. All mutating file operations (copy/move/delete/...) run through
the Rust operation engine in `fm-operations`/`fm-application` as cancellable, resumable jobs with
conflict policies — never implemented in TypeScript (spec rule 6/8; see also
[ADR 0004](docs/decisions/0004-vfs-provider-abstraction.md) and
[ADR 0005](docs/decisions/0005-operation-scheduler-and-conflict-handling.md)).

### Plugins

Restricted, resource-limited Lua sandbox (`fm-plugin-runtime`, `fm-plugin-api`) with per-plugin
diagnostics and auto-disable on repeated failure — never expose arbitrary filesystem methods,
unrestricted APIs, or native dynamic libraries as the plugin ABI (see
[ADR 0006](docs/decisions/0006-plugin-runtime-selection.md)).

### Generated code — never hand-edit

`frontend/openapi/openapi.json` (via `pnpm api:export`) and the Orval-generated Fetch client under
`frontend/src/api/` (via `pnpm api:generate`) are checked into git and regenerated, not edited;
`pnpm api:check` fails CI when either is stale relative to the backend.

## Conventions (spec §35)

- Small, reviewable commits; no speculative abstractions not tied to a planned feature.
- Strongly typed errors: `thiserror` in libraries, `anyhow` only in `apps/*` binaries.
- Cancellation for long-running work; never test destructive operations outside temp roots.
- Preserve browser/Tauri parity — don't build a feature that only works in one host.
- No React/Vue/Svelte/Angular libraries, and no generic state framework without demonstrated need.
- Don't render large directories without virtualization; don't silently overwrite files or follow
  symlinks.
- Before finishing: run `pnpm run lint` and the relevant tests; report incomplete or
  platform-untested behaviour explicitly rather than silently.

## Release process

Version is workspace-wide via `[workspace.package].version` in the root `Cargo.toml` (MSI/WiX only
accepts numeric pre-release identifiers, hence `0.1.0-1`, `0.1.0-2`, ... not `0.1.0-alpha.1`).
Flow: bump version → commit → push → wait for `main` CI green → tag (`vX.Y.Z-N`, matching the
Cargo version) → push tag → `release-desktop.yml` builds macOS/Windows/Linux, publishes the GitHub
Release, updates the Homebrew tap, and attempts a Chocolatey push. That push has failed with an
upstream 504 on every release so far ([chocolatey/home#264](https://github.com/chocolatey/home/issues/264))
— the built `.nupkg` is still attached to the GitHub Release as a fallback for a manual push, so
treat the automated failure as a known non-blocking issue, not something to keep re-diagnosing.
