# 0003 OpenAPI source of truth and generated TypeScript client

Status: accepted

## Context
The REST API (ADR [0002](0002-axum-rest-and-sse.md)) needs a typed TypeScript client, and the
backend and frontend must not drift apart on request/response shapes as the API grows (spec §8,
§9, §10).

## Decision
`utoipa` annotations on the Axum handlers and DTOs are the single source of truth for the API
shape. `pnpm run api:export` (`fm-server export-openapi`) generates `frontend/openapi/openapi.json`
from those annotations, and `pnpm run api:generate` runs Orval against that document to produce the
Fetch-based client under `frontend/src/api/generated/`. Both generated artefacts are committed to
git, and `pnpm run api:check` fails CI if either is stale relative to the backend (AGENTS.md,
`file-manager-coding-agent-spec.md` §9, §10).

## Alternatives
- **Hand-written TypeScript types and fetch calls**: rejected — the exact drift problem this
  decision exists to prevent; every backend DTO change would need a manual, easy-to-forget mirror
  edit.
- **Schema-first (write `openapi.json` by hand, generate Rust types from it)**: rejected — `utoipa`
  already derives the schema from the same Rust types the handlers use, so code-first avoids a
  second parallel definition of the DTOs.
- **tRPC-style shared-types-over-the-wire**: rejected — ties the browser transport to a
  Rust-specific RPC mechanism and does not fit the Tauri adapter (ADR 0001) at all.

## Consequences
- Generated code (`frontend/openapi/openapi.json` and `frontend/src/api/` client) must never be
  hand-edited; fixes go into the `utoipa` annotations or the Orval config, then both commands are
  re-run.
- Adding or changing an endpoint is a three-step commit: annotate the handler/DTO, run
  `api:export` + `api:generate`, then commit both generated files alongside the handler change.
- `api:check` gives CI a cheap, deterministic way to catch a forgotten regeneration without needing
  to re-derive the schema itself.

## Revisit conditions
Revisit if `utoipa`'s derive coverage stops matching the DTO shapes the API needs (forcing manual
schema overrides to accumulate), or if Orval's generated client shape becomes a poor fit for the
`FileManagerClient` interface it needs to satisfy.
