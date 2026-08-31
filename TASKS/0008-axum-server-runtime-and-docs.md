# 0008 Axum server with runtime capabilities, OpenAPI JSON and Swagger UI

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: backend
Depends on: 0007

## Context
`file-manager-coding-agent-spec.md` §2.2, §8, §9, §21 and §33 step 2. First running backend:
health, runtime capabilities, OpenAPI document and Swagger UI. Handlers must stay thin (§3 rule 2).

## Acceptance Criteria
- `apps/fm-server` starts an Axum server bound to loopback by default with a configurable port.
- Routes: `GET /api/v1/health`, `GET /api/v1/runtime`, `GET /api/v1/openapi.json`,
  `GET /api/v1/docs` (Swagger UI).
- `GET /api/v1/runtime` returns the `RuntimeCapabilities` shape from §21 with
  `runtime: "browserServer"` and the detected platform; unimplemented natives report `false`.
- `tower-http` layers for tracing, request body limits and CORS (no wildcard origin — §22).
- Every response carries a correlation/request id, also included in error bodies (§8).
- `tracing` initialised with env-filter; structured fields include request id and duration (§30).
- `fm-application` exposes a `FileManagerService` facade (§7) and the Axum handler only maps
  request → service call → DTO; no filesystem logic in `apps/fm-server`.
- Integration test using `axum::serve` on an ephemeral port asserts 200 + JSON shape for
  `/api/v1/health` and `/api/v1/runtime`, and that `/api/v1/openapi.json` parses as OpenAPI 3.1.

## Implementation Notes
- Use `utoipa-axum`'s router integration so routes and schemas cannot drift.
- Operation ids: `getHealth`, `getRuntimeCapabilities` (§9 naming rules).
- Server config (bind address, CORS origins, roots) belongs in a typed config struct now, so 0064
  can harden it without restructuring.

## Agent Notes
- 2026-07-29 claude: Implemented the first running Axum host.
  - `fm-application`: added `service::FileManagerService` (spec §7) with a
    single method, `runtime_capabilities()`, and `error::ApplicationError`
    (`thiserror`, per AGENTS.md) with `code()`/`into_dto(request_id)`. The
    facade intentionally does not stub the remaining §7 fields
    (workspaces/directories/operations/actions/plugins/events) — those crates
    don't exist yet, so adding empty fields now would be speculative; they
    land incrementally as their tasks are implemented. Platform is detected
    via `std::env::consts::OS` rather than the `fm-platform` crate, which
    remains a stub reserved for task 0058's richer capability trait.
  - `fm-transport-dto`: added `health::{HealthDto, HealthStatusDto}` (not
    part of task 0007, needed here for `GET /api/v1/health`), with round-trip
    and exact-shape tests.
  - `apps/fm-server`: gained a `lib.rs` alongside `main.rs` (`config`,
    `state`, `routes::{health, runtime}`, `error`) purely so the integration
    test can build the exact same `Router` that `main` serves via
    `fm_server::build_router`. `main.rs` owns CLI/env parsing (`clap`, with
    an `env` feature) and tracing-subscriber initialisation.
  - Routes registered through `utoipa_axum::router::OpenApiRouter` +
    `routes!` so paths/schemas cannot drift; operation ids `getHealth` and
    `getRuntimeCapabilities` per §9. `GET /api/v1/docs` and
    `/api/v1/openapi.json` are both registered by merging
    `utoipa_swagger_ui::SwaggerUi::new(...).url(...)` into the router.
  - Request correlation ids use `tower_http::request_id`'s own
    `SetRequestIdLayer`/`PropagateRequestIdLayer`/`MakeRequestUuid` (no
    hand-rolled middleware): every response carries `x-request-id`, and the
    404 fallback handler parses it back into a `Uuid` to populate
    `ApplicationErrorDto.request_id`.
  - Middleware is applied as separate `Router::layer()` calls rather than one
    composed `tower::ServiceBuilder`, because `CorsLayer` requires a response
    body implementing `Default` and `RequestBodyLimitLayer` produces a body
    type that doesn't; each `Router::layer()` call re-erases the response
    back to `axum::body::Body`, so `RequestBodyLimitLayer`, the
    request-id/trace stack, and `CorsLayer` are three separate calls
    (in that order, `CorsLayer` outermost).
  - `CorsLayer`'s allow-list defaults to empty (blocks every cross-origin
    request) and is populated only from the repeatable `--cors-origin`
    flag/`FM_SERVER_CORS_ORIGIN` env var — never a wildcard (spec §22).
  - `ServerConfig.roots` is parsed from CLI/env and logged at startup (not
    yet consumed by a VFS, which doesn't exist yet); kept as a real,
    used field rather than dead code, ready for task 0064 to harden.
  - `HealthDto` is built directly in the handler rather than through
    `FileManagerService`, since it reports process liveness, not a user
    intention or domain state; `RuntimeCapabilitiesDto` does go through the
    service, matching the acceptance criteria's wording.
  - `clipboard: true` while every native OS capability reports `false`: the
    browser Clipboard API needs no native bridge, so it isn't one of the
    "unimplemented natives" the acceptance criteria refers to.
  - New workspace dependencies: `tokio` gained the `net` feature; `tower`,
    `tower-http` (`trace`, `limit`, `cors`, `request-id`, `util`),
    `utoipa-axum` (0.2.0), `utoipa-swagger-ui` (9.0.2, `axum` feature), and
    `reqwest` (fm-server dev-dependency only, `default-features = false`,
    `rustls-tls`) were added; `clap` gained the `env` feature.
  - Verified: `cargo test -p fm-application -p fm-transport-dto -p fm-server`
    — fm-application 5/5, fm-transport-dto 29/29 (27 existing + 2 new health
    tests), fm-server 5/5 integration tests (health, runtime shape, OpenAPI
    3.1 parse + path presence, Swagger UI 200, request-id on both success and
    404 paths). `cargo test --workspace` green throughout, including the
    architecture fitness test. `cargo fmt --all --check` and
    `cargo clippy --workspace --all-targets -- -D warnings` both clean.
    Manually ran `cargo run -p fm-server -- --port <n>` and curled
    `/api/v1/health`, `/api/v1/runtime` and `/api/v1/openapi.json` to confirm
    real request/response behaviour (platform detected as `macos`,
    `x-request-id` present on every response).
  - Known gaps: none against this task's literal acceptance criteria.
    `FileManagerService` exposes only `runtime_capabilities()`; the rest of
    the §7 facade (open_workspace, navigate, start_operation, ...) is
    deliberately deferred to the tasks that build their backing services.
