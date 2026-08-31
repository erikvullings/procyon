# 0091 Native file icon overlay (backend-served, layered over 0085)

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: backend,frontend
Depends on: 0085, 0059

## Context
Split out of task 0085 ("do both, layered" design) because the combined scope was too large for
one reviewable PR. 0085 shipped the mandatory frontend baseline: a themeable icon registry
(`frontend/src/features/directory-table/entry-icons.ts`) resolving `EntryKind` +
extension/MIME type to an SVG glyph, fully overridable by a theme package, with no backend
dependency. That baseline is the permanent default rendering path and already satisfies the
user's original complaint ("I can't see any icons").

This task is the enhancement layer: when `runtimeCapabilities.nativeFileIcons` is true, overlay the
real OS icon (already fetchable server-side, task 0059/0060) on top of the themed glyph, falling
back to the themed glyph while loading/unavailable/on non-native hosts. Nothing in 0085's frontend
baseline needs to change for this — it stays the fallback, always-available layer underneath.

### What already exists (do not re-derive/re-implement)
- `crates/fm-platform/src/adapter.rs`'s `PlatformAdapter::file_icon(&self, path: &Path) ->
  Result<Vec<u8>, PlatformError>` — default `Unsupported`.
- `crates/fm-platform-macos/src/lib.rs` implements it: caches PNG bytes in an in-memory
  `Mutex<HashMap<String, Vec<u8>>>` keyed by `icon_cache_key(path)` (lowercased extension, or a
  sentinel for directories/extension-less files) — i.e. **already one lookup per extension, not
  per file** (spec §28). Reuse this cache; do not add a second cache layer on top of it.
- `crates/fm-platform-windows/src/lib.rs` does **not** implement `file_icon` (delegates to the
  fallback, i.e. always `Unsupported`) — Windows native icons are out of scope here too (0060
  territory). Design the new route/command so an `Unsupported` result cleanly maps to "no icon
  available, caller should fall back" on every host, not just macOS.
- `RuntimeCapabilitiesDto.nativeFileIcons` (`crates/fm-transport-dto/src/runtime.rs`) is already
  populated end-to-end (`crates/fm-application/src/service.rs::runtime_capabilities()`) and
  surfaced identically by `getRuntimeCapabilities()` on both the HTTP and Tauri frontend clients.
  Nothing to add here — just consume it.
- The existing platform-action pattern for turning a wire location into a native path:
  `FileManagerService::invoke_platform_action` (`crates/fm-application/src/service.rs`) does
  `Location::parse(uri)?.to_native_path()?`, never hand-building or shell-interpolating the path.
  Follow the same pattern for the new `file_icon` service method (parse a `uri` query/field the
  same way, then call `self.platform.file_icon(&path)`).
- `fn map_platform_error(action_id: &ActionId, error: PlatformError) -> ApplicationError` in
  `service.rs` is specific to action invocation (needs an `ActionId` to build its message) — do not
  reuse it as-is. Write a small dedicated mapping instead: `PlatformError::Unsupported` should map
  to `ApplicationError::NotFound` (no icon available for this extension on this host — a normal,
  expected, silently-recoverable outcome, not a 5xx), any other `PlatformError` to
  `ApplicationError::PlatformOperationFailed(message)` (matches `apps/fm-server/src/error.rs`'s
  existing `status_for` mapping: 404 and 502 respectively).

## Design (resolved — do not re-litigate)
- **Route shape**: `GET /api/v1/icons?uri=<percent-encoded location>` (a query parameter carrying
  one concrete example location for the target extension, not a full entry id — there is no entry
  registry the backend can resolve an opaque id through, same reasoning as
  `invoke_platform_action`). The frontend already knows a real entry's `location` when it wants an
  icon for e.g. `.pdf`. Cache-per-extension is enforced by the platform adapter layer already: the
  route/service is just a thin pass-through and does not need its own extension-keyed cache.
- **New `FileManagerService` method**: `pub fn file_icon(&self, uri: &str) -> Result<Vec<u8>,
  ApplicationError>` — parses `uri` via `Location::parse` + `to_native_path`, calls
  `self.platform.file_icon(&path)`, maps errors per the dedicated mapping above. Add near
  `runtime_capabilities()`.
- **HTTP route** (`apps/fm-server/src/routes/icons.rs`, new file, registered in `routes/mod.rs` and
  `lib.rs`'s `api_router()`): a `Query<FileIconQuery>` extractor (`{ uri: String }`,
  `#[derive(Deserialize, IntoParams)]`) and a handler returning `impl IntoResponse` — on success,
  `(StatusCode::OK, [(header::CONTENT_TYPE, "image/png")], bytes)`; on failure, the existing
  `ApiError` (`apps/fm-server/src/error.rs`) so status codes/error bodies stay consistent with
  every other route. utoipa 5.5.0 (see root `Cargo.toml`) accepts a `responses(...)` entry with
  just `content_type = "image/png"` and no `body` for a raw-bytes success response — confirm the
  exact attribute syntax against the installed utoipa version if it doesn't compile as expected,
  erring toward simplicity over perfect OpenAPI annotation fidelity (no existing route in this
  codebase returns raw bytes yet; this is the first).
- **Tauri command** (`apps/fm-desktop/src-tauri/src/commands.rts`): `get_file_icon(state, uri:
  String) -> Result<Vec<u8>, ApplicationErrorDto>`, same shape as every other command in that file
  (`.map_err(|error| error.into_dto(Uuid::new_v4()))`), registered in `lib.rs`'s
  `invoke_handler!`. Investigated during 0085: no existing base64/binary-IPC precedent anywhere in
  `apps/fm-desktop`, and no `base64` crate in the workspace dependency list. Returning `Vec<u8>`
  directly (Tauri JSON-serializes it as a number array) is an acceptable, simple starting point
  given icon payloads are small (single-digit KB PNGs) — do not add a `base64` dependency purely
  for this unless a real IPC payload-size problem shows up in practice.
- **OpenAPI/Orval regen**: `pnpm run api:export && pnpm run api:generate` from the repo root once
  the new route compiles. Never hand-edit `frontend/openapi/openapi.json` or anything under
  `frontend/src/api/generated/`.
- **Frontend binary-response risk (read before starting)**: `frontend/src/api/fetch-mutator.ts`
  (the Orval fetch mutator every generated request function delegates to) currently always calls
  `response.text()` in `readBody()` and only `JSON.parse`s it when the content-type is
  `application/json` — otherwise it returns the *string* from `.text()`. Decoding a binary PNG
  response through `.text()` will corrupt the bytes (lossy UTF-8 decoding). This needs a real fix,
  not a workaround in a single client method:
  - Either extend `readBody()` to special-case a binary content-type (e.g. `image/png` → read via
    `response.arrayBuffer()` / `.blob()` instead of `.text()`), gated so every other (JSON) endpoint
    is completely unaffected, or
  - Bypass the generated Orval function entirely for this one endpoint and call `fetch()` directly
    in `HttpFileManagerClient` (still going through `resolveBaseUrl()`/session header helpers from
    `fetch-mutator.ts` if they're exported, or duplicating the minimal necessary bit) — acceptable
    since Orval's generated function signature for a raw-bytes response is unlikely to be useful
    here anyway.
  - Whichever approach is chosen, add a regression test asserting the *other* existing
    `fetch-mutator.test.ts` JSON-handling behaviour is unchanged.
- **`FileManagerClient` interface** (`frontend/src/api/client/file-manager-client.ts`): add
  `getFileIcon(sampleLocationUri: string, signal?: AbortSignal): Promise<Uint8Array | undefined>`
  — returns `undefined` on any error/unsupported/not-found rather than throwing (caller's fallback
  contract is "never a broken image, always fall back to the themed glyph"). Implement on **all
  three** adapters:
  - `HttpFileManagerClient`: per the binary-response fix above; catch/swallow errors to `undefined`.
  - `TauriFileManagerClient`: `invoke<number[]>('get_file_icon', { uri })` (or whatever binary
    shape was chosen), converted to `Uint8Array`; catch/swallow errors to `undefined`.
  - `MockFileManagerClient` (`frontend/src/api/client/mock-file-manager-client.ts`): check its
    existing style for faking per-method behavior (it already fakes `nativeFileIcons: false` in its
    default `getRuntimeCapabilities()`); add a small deterministic/configurable fake (e.g. a tiny
    built-in 1x1 PNG for a configurable allow-list of extensions, `undefined` otherwise) so tests
    can assert both the native-icon-present and fallback paths.
- **Directory-table wiring**: a small loader/hook module (e.g.
  `frontend/src/features/directory-table/native-icon-loader.ts`) that, only when
  `runtimeCapabilities.nativeFileIcons` is true, lazily calls `client.getFileIcon(...)` on first
  render of a given extension, caches the result **in-memory only** (a `Map<string, Uint8Array |
  undefined>`, no persistence across reloads — matches the backend's own cache lifetime), and
  overlays the decoded image (e.g. an `<img>` with an object URL, or a `data:` URI) in place of the
  themed glyph from 0085 once loaded. Every failure path (fetch error, `Unsupported`, still loading,
  non-native host) must render the exact same themed-glyph fallback from `entry-icons.ts` — never a
  broken image or blank cell. Remember to revoke any object URLs you create to avoid leaking blob
  URLs as rows scroll in/out.

## Acceptance Criteria
- New HTTP route + Tauri command both call the existing `PlatformAdapter::file_icon`, preserving
  its one-lookup-per-extension cache behaviour (no new duplicate cache layer added on top).
- `pnpm run api:check` is clean (regenerated OpenAPI + Orval client committed, not stale).
- `FileManagerClient.getFileIcon` implemented and tested on all three adapters (http/tauri/mock).
- The directory table overlays a native icon when available and falls back to the 0085 themed glyph
  silently on any failure/unsupported host/while loading — verified by a test for each path.
- Works identically in `pnpm dev:http` (browser) and `pnpm dev:tauri` (desktop) — a host without the
  capability only ever shows 0085's themed glyphs, which remains a fully-functional default.
- Existing `fetch-mutator.test.ts` JSON-handling assertions still pass unchanged (binary-response
  handling must not regress every other endpoint using the same mutator).
- Tests: a Rust test for the new route/command (extension-keyed cache behaviour end-to-end,
  `Unsupported` → 404 / clean error mapping on a host without the capability — the CI runner is
  Linux, so exercise this via the `FallbackPlatformAdapter` path, not by asserting real macOS icon
  bytes), and frontend tests for the lazy-fetch/cache/fallback behaviour of the loader module.

## Agent Notes
- 2026-08-03 Claude Sonnet 5 (Copilot): Split out of 0085 during that task's implementation. Not
  started. The Design section above already resolves the open questions 0085 flagged (route shape,
  service method, error mapping, Tauri payload shape, and — found during 0085's own investigation —
  a real correctness risk in `fetch-mutator.ts`'s binary-response handling that must be fixed as
  part of this task, not worked around).
- 2026-08-04 Codex: Implemented the application service, PNG HTTP route, Tauri command, regenerated
  OpenAPI/Orval client, binary-safe fetch mutator, all three frontend transports, and a
  capability-gated lazy loader that caches by normalized extension and preserves the 0085 themed
  fallback during loading or failure. `pnpm run lint`, frontend tests (489 passed, 1 skipped),
  script tests (29 passed), route integration tests, and the Tauri command test pass. OpenAPI and
  Orval regeneration is byte-identical. The full `cargo test --workspace` run is otherwise green
  but remains blocked by unrelated concurrent icon-theme work: `fm-plugin-runtime`'s
  `discovers_the_real_catppuccin_icons_plugin_package` expects 27 definitions while the package now
  contains 31. Native icon bytes were exercised on macOS by the existing platform-adapter tests;
  Windows was not manually exercised and correctly retains the themed fallback through its false
  capability/unsupported adapter.
