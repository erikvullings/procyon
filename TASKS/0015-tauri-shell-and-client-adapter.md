# 0015 Tauri 2 shell application and Tauri client adapter

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: desktop
Depends on: 0012, 0014

## Context
`file-manager-coding-agent-spec.md` §2.4, §11, §12 and §33 steps 1 and 3. The desktop host embeds
the same frontend and calls the same `FileManagerService` through thin command adapters.

## Acceptance Criteria
- `apps/fm-desktop/src-tauri` builds a Tauri 2 application embedding the Vite frontend; the window
  shows the same app as browser mode.
- Commands mirror the semantic API: at minimum `get_runtime_capabilities` and `navigate_pane`,
  implemented as thin wrappers over `FileManagerService` (§11) with no filesystem logic.
- `RuntimeCapabilities.runtime` is `"tauri"` in desktop mode and reports the real platform.
- Errors map to `ApplicationErrorDto`, identical in shape to the REST errors.
- `frontend/src/api/client/tauri-file-manager-client.ts` implements `FileManagerClient` via
  `invoke`, and `frontend/src/api/events/tauri-event-stream.ts` implements `EventStream` via Tauri
  channels/events.
- Tauri capabilities file allow-lists only the commands actually used; no default full-filesystem
  plugin access (§22).
- Normal desktop mode opens no localhost port; any diagnostics HTTP mode is off by default (§11).
- `pnpm dev:tauri` and `pnpm build:tauri` work on macOS; CI adds Tauri build jobs for macOS and
  Windows (§31).
- A smoke test asserts the app starts and `getRuntimeCapabilities()` returns `runtime: "tauri"`.

## Implementation Notes
- Do not start Axum inside the Tauri process to reuse HTTP (§11).
- Keep `apps/fm-desktop/src-tauri` dependent on `fm-application` only, never on `fm-server`.
- Icons and product metadata can be placeholders until 0063.

## Agent Notes
- 2026 claude: Implemented the Tauri 2 shell (`apps/fm-desktop/src-tauri`) and the frontend Tauri
  adapter.
  - Root `Cargo.toml`: added `apps/fm-desktop/src-tauri` to workspace `members` plus
    `exclude = ["apps/fm-desktop"]` (Tauri nests the Rust crate one level deeper than the
    `apps/*` glob reaches), and `tauri`/`tauri-build` to `[workspace.dependencies]`.
  - `apps/fm-desktop/src-tauri/{Cargo.toml,build.rs,main.rs,lib.rs,commands.rs,tauri.conf.json,
    capabilities/default.json,icons/*}` (new): a Tauri 2 application depending only on
    `fm-application` + `fm-transport-dto` (never `fm-server`), with one real command,
    `get_runtime_capabilities`, thinly wrapping `FileManagerService::runtime_capabilities()`.
    `capabilities/default.json` allow-lists only `core:default` (verified to grant no fs/shell
    access) plus the app's own command — no default full-filesystem plugin access. No Axum is
    started in-process; the app opens no localhost port.
  - `navigate_pane` is **not** implemented: `FileManagerService` has no `navigate` method yet
    (directory listing lands in tasks 0018/0019), so there is nothing to thinly wrap without
    inventing filesystem logic ahead of its owning task. Documented in `commands.rs` and mirrored
    by `TauriFileManagerClient.navigatePane` throwing `NotImplementedError` naming task `0019`
    (same as `HttpFileManagerClient`).
  - A `tauri::test`-based smoke test (`app_starts_and_reports_the_tauri_runtime`) asserts the app
    starts (mock runtime, real `tauri.conf.json`/capabilities via a shared `build_context()`
    helper) and `get_runtime_capabilities` returns `runtime: "tauri"` via the mock-runtime IPC
    path (`InvokeRequest.url = "tauri://localhost"`, the macOS/Linux local-origin scheme — the
    Windows-style `http://tauri.localhost` from Tauri's own docs.rs example fails silently on
    macOS/Linux with a confusing ACL rejection; see `/memories/repo/fm-tauri-conventions.md`).
  - `crates/fm-test-support/src/architecture.rs`: added `("fm-desktop", 4)` to `CRATE_LAYERS`
    (same layer as `fm-cli`/`fm-server` — a host application with the same dependency shape),
    fixing the `workspace_crates_respect_the_documented_layering` fitness test.
  - `frontend/src/api/client/tauri-file-manager-client.ts`: `getRuntimeCapabilities` now calls
    `invoke('get_runtime_capabilities')` from `@tauri-apps/api/core`. Every other method throws
    `NotImplementedError` naming its owning task (`0019`/`0036`/`0049`/`0053`/`'TBD'` for
    `getWorkspace`, matching `http-file-manager-client.ts`'s per-method pattern rather than one
    crate-wide task constant). `subscribe()` wires listeners through the new `TauriEventStream`
    skeleton and calls `connect()`.
  - `frontend/src/api/events/tauri-event-stream.ts` (new): a minimal `EventStream` implementation
    — `connect()`/`close()` flip a `status` observable between `'open'`/`'closed'`, and
    `listeners` is a working `BackendEventListenerRegistry` — enough to satisfy the interface and
    let `subscribe()` work end-to-end. **No backend forwarding is wired up yet**: nothing
    subscribes to the Rust `EventBus` or listens on a Tauri channel/event. That full parity work
    (EventBus subscription, SSE-identical payloads, parity test suite) is task 0034's explicit
    scope, which itself depends on 0033's SSE client landing first — tracked via `TODO(0034)`
    comments in both files.
  - `frontend/package.json`: added `@tauri-apps/api` dependency. Root `package.json`: added
    `@tauri-apps/cli` devDependency and rewired `dev:tauri`/`build:tauri` from
    `not-implemented.sh` stubs to `cd apps/fm-desktop/src-tauri && pnpm exec tauri dev|build`
    (must `cd` first since `tauri.conf.json`'s `beforeDevCommand`/`frontendDist` paths are
    relative to `src-tauri`). Verified `pnpm exec tauri --version` resolves correctly from that
    directory; did **not** run a full `pnpm dev:tauri`/`build:tauri` end-to-end (no GUI/display
    in this sandboxed environment to verify the window actually opens on macOS).
  - `.github/workflows/ci.yml`: added a `desktop` job (`macos-latest`, `windows-latest` matrix)
    running `pnpm run build:tauri`, matching the spec's CI requirement. **Not verified by an
    actual CI run** — added structurally and checked against
    `scripts/ci-workflow.test.mjs` (10/10 still passing), but GitHub Actions itself was not
    exercised.
  - Added tests: `tauri-file-manager-client.test.ts` (invoke happy-path/error-propagation,
    per-method `NotImplementedError` + task references, `subscribe()` wiring) and
    `tauri-event-stream.test.ts` (status transitions, listener dispatch/unsubscribe).
  - Verification: `cargo test --workspace` (all green, including the fixed architecture fitness
    test), `cargo clippy --workspace --all-targets -- -D warnings` (clean), `cargo fmt --all
    --check` (clean), `pnpm --dir frontend run typecheck` (clean), `pnpm --dir frontend test`
    (57/57 passing), `pnpm exec biome check` on all touched frontend files (clean).
  - Known gaps carried forward (all explicitly owned by later tasks, not silently dropped):
    `navigate_pane` command (0019), full `tauri-event-stream.ts` EventBus/channel forwarding and
    SSE parity (0034), `getWorkspace` (no owning task yet — `'TBD'`, same gap as the HTTP
    client), and live verification of `pnpm dev:tauri`/`build:tauri`/the new CI `desktop` job
    (none of which can be exercised in this sandboxed, headless, non-macOS-GUI environment).
