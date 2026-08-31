# 0064 Browser/server mode security hardening

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0032, 0044

## Context
`file-manager-coding-agent-spec.md` §22 — the browser/server backend controls files and must not run
as an unauthenticated localhost API. This must be in place before the server mode is used for
anything beyond local development.

## Acceptance Criteria
- Loopback-only binding by default; LAN binding requires an explicit opt-in flag and logs a warning.
- A randomly generated session secret per run (persisted only where the deployment configures it),
  with authenticated sessions required for all `/api/v1` routes including SSE (§10, §22).
- Strict origin validation and a non-wildcard CORS policy.
- Request-size limits and rate limiting on mutating endpoints.
- Configured accessible roots: every incoming `Location` is validated to resolve inside an allowed
  root after normalization and symlink resolution; absolute paths from clients are never trusted
  (§22).
- TLS supported for remote access, with documentation on how to enable it.
- Audit logging for destructive operations (delete, trash, overwrite) including who, what and when —
  without file contents or secrets (§30).
- Separate server-mode configuration file/section, distinct from desktop defaults.
- Security tests: path-traversal attempts (`..`, encoded, symlink escape, UNC, `\\?\`), unauthorised
  REST and SSE access, disallowed origin, oversized request.
- `docs/architecture/security.md` documents the model and the dev-mode relaxations.

## Implementation Notes
- Development mode may relax auth, but the relaxation must be explicit, logged at startup and
  impossible when a non-loopback bind is configured.
- Tauri mode uses OS user permissions but still restricts exposed commands via capabilities (§22).

## Agent Notes
- 2026-08-13: An earlier pass (untracked in this file) had built the security primitives
  (`fm-server/src/{config,auth,accessible_roots,audit}.rs`) as standalone, unit-tested modules and
  written `docs/architecture/security.md` describing them as active, but none of them were wired
  into the live Axum router — the API was fully open. This pass wires them in and closes the gap:
  - `require_session` is now layered onto `build_router` as real middleware, covering every
    `/api/v1` route including SSE (`GET /api/v1/events`, via `?token=` since `EventSource` can't
    set headers); only `/api/v1/health` and the Swagger/OpenAPI surface stay open. `fm-server`
    prints an access token to stdout at startup (unless `--dev-mode-auth-disabled`).
  - `accessible_roots::validate_location` is called from every route handler that accepts a
    `Location` (`directories/list|refresh`, `navigation/open`, `entries/metadata`, `files/range`,
    `files/editable/load|save`, `files/search`, `operations` sources/destination(s), `search`
    roots) via `crate::error::require_within_roots`, rejecting with 403 before the request reaches
    `FileManagerService`. `validate_within_accessible_roots` now also handles not-yet-existing
    paths (create/rename targets) by canonicalizing the nearest existing ancestor.
  - `AuditEvent::log` is now called from `start_operation` (delete/trash/overwrite-on-conflict),
    `resolve_operation_conflict` (overwrite resolution) and `save_editable_file`, using a 6-byte
    SHA-256 fingerprint of the caller's token as `session_id` (never the raw token).
  - Added rate limiting (`fm-server/src/rate_limit.rs`, `governor`-backed token bucket) on
    mutating methods, configurable via `--max-mutations-per-second` (default 20/s), returning 429.
  - Added a server-mode TOML config file (`ServerFileConfig` in `config.rs`, loaded via
    `--config`/`FM_SERVER_CONFIG`), fully separate from the desktop app's settings directory, with
    CLI/env values taking precedence over the file.
  - Added direct TLS termination (`--tls-cert`/`--tls-key`, `axum-server` + `rustls`) alongside the
    existing reverse-proxy option; both flags must be set together or startup panics.
  - Added real end-to-end HTTP security tests in `tests/security.rs` (`http_security_tests`
    module, 10 new `#[tokio::test]`s driving the actual router: unauthorized REST/SSE rejected,
    dev-mode bypass, disallowed-origin CORS preflight, oversized body → 413, path outside/inside
    accessible roots → 403/200, mutation rate limit → 429) alongside the pre-existing pure-logic
    unit tests, which now exercise the wiring instead of only the standalone functions.
  - Updated `docs/architecture/security.md` to describe the model as actually implemented (token
    delivery, rate limiting, config file, direct TLS) rather than the aspirational draft it was.
  - Verified: `cargo test -p fm-server --no-fail-fast` — 134 of 135 tests across lib + every
    integration file pass; the one failure (`plugin_routes.rs`) is a pre-existing, unrelated plugin
    id mismatch from an in-progress icon-theme rename, confirmed present on `main` before this
    change via `git stash`. `cargo clippy -p fm-server --all-targets` is clean, zero warnings.
  - Known, documented gap: `POST /api/v1/workspaces/{id}/commands` (`addTab`/`navigateTab`) carries
    `Location` values in tab/history state but isn't validated against accessible roots at that
    handler — a workspace command's location only reaches the filesystem through a subsequent
    `directories/list`/`navigation/open` call, which *is* validated, so this isn't a bypass, but
    tightening the command handler directly is a reasonable follow-up. Noted in
    `docs/architecture/security.md` §4.
  - Not attempted: per-client (vs. server-wide) rate limiting — the server-wide limiter matches the
    single-operator loopback deployment this task targets; a multi-tenant deployment behind a
    reverse proxy should add per-IP limiting there instead (documented in the security doc).
- 2026-08-13: Closed the frontend half of the gap noted above. The backend enforced tokens but the
  frontend never sent one, so `README.md`'s "Running fm-server on a remote host" section (added
  earlier this session) initially had to document that fact as a known limitation. Fixed instead of
  left documented:
  - `frontend/src/api/session-token.ts` (new): `sessionStorage`-backed token storage (never
    `localStorage`, per the security doc's own recommendation), with an in-memory cache so reads
    don't hit storage on every request.
  - `frontend/src/api/fetch-mutator.ts`: added `setUnauthorizedHandler()`, invoked on every `401`
    response before the `ApiError` propagates, so the app can react (clear the stale token,
    re-prompt) without every call site special-casing 401.
  - `frontend/src/api/events/sse-event-stream.ts`: `SseEventStreamOptions.tokenProvider` appends
    `?token=` on every (re)connect, since browser `EventSource` can't set an `Authorization` header
    — re-read fresh each attempt so a token entered after construction, or rotated between
    reconnects, is picked up.
  - `frontend/src/api/client/http-file-manager-client.ts`: wires `getSessionToken` in as the SSE
    stream's `tokenProvider`.
  - `frontend/src/app/session-token-gate.ts` (new): the one UI surface that collects the token,
    gating `AppShell` behind a "Sign in to fm-server" form. Deferred child rendering (`children: ()
    => m.Children`) means `AppShell`'s `oninit` — and its first API calls — never run until the
    token requirement is resolved, not just visually hidden.
  - **Caught before shipping**: an initial version of the gate always prompted for a token,
    including under `--dev-mode-auth-disabled` — but dev mode never prints a token (there's nothing
    to enforce), so that would have permanently blocked the documented local-dev flow. Fixed by
    having the gate probe a harmless authenticated endpoint (`GET /api/v1/runtime`) once on mount:
    a `200` means auth isn't enforced and the gate steps aside for the session; a `401` shows the
    prompt. Verified against a real running `fm-server` in both modes via the Browser pane, not
    just unit tests: with `--dev-mode-auth-disabled` the app loads directly with no prompt; without
    it, the prompt appears, the printed token is accepted, and an authenticated directory listing
    (with live SSE events) renders end-to-end.
  - Verified: `pnpm vitest run` — 999 of 999 frontend tests pass (25 new across
    `session-token.test.ts`, `fetch-mutator.test.ts`, `sse-event-stream.test.ts`, and
    `session-token-gate.test.ts`). `tsc --noEmit` and `biome check` are clean.
  - Updated `README.md`'s remote-host section (step 3) and dev-flow note to describe the
    implemented behavior instead of the gap; removed the now-stale "not yet implemented" callout.
  - Added a `fm-frontend-http` entry to `.claude/launch.json` for future browser-driven verification
    of the HTTP runtime against a real `fm-server` (the existing entry was mock-only).
  - Unrelated blocker hit and resolved separately (not part of this task): the pre-commit hook's
    `cargo test --workspace` failed on `tests::discovers_the_real_catppuccin_icons_plugin_package`
    because a just-committed expanded icon theme (`plugins/catppuccin-icons/icon-theme.json`, from
    a separate, unrelated commit) declared a `fileNames` map that `IconThemeManifest` didn't
    support and denied as an unknown field. Fixed in its own commit
    (`feat(plugins): support exact file-name icon-theme mappings`), matching the design already
    in progress on `feature/0060-windows-platform-integration` (case-sensitive file-name matching,
    since the theme relies on `Cargo.lock` vs `cargo.lock` resolving differently) rather than
    inventing a divergent one.
