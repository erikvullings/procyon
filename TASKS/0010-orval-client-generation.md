# 0010 Orval-generated Fetch client and api:check

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: frontend
Depends on: 0009

## Context
`file-manager-coding-agent-spec.md` §2.3 and §33 step 2. Generate a Fetch-based TypeScript client
from the exported OpenAPI document. Generated files are never edited by hand (§35).

## Acceptance Criteria
- `frontend/orval.config.ts` configured for: Fetch client, TypeScript DTOs, split output into
  `frontend/src/api/generated/`, and a custom mutator — explicitly **not** React Query.
- `frontend/src/api/fetch-mutator.ts` implements: base URL resolution, JSON handling,
  `AbortSignal` cancellation pass-through, optional auth/session header, and mapping non-2xx
  responses to a typed `ApiError` carrying `code`, `message`, `requestId` and `details` (§8).
- `pnpm api:generate` regenerates the client; generated output is checked into git.
- `pnpm api:check` (export + generate + `git diff --exit-code`) passes locally and runs in CI,
  failing when the checked-in document or client is stale (§2.3, §31).
- `frontend/src/api/generated/` carries a header/README stating it is generated and must not be
  edited, and is excluded from lint autofix and formatting churn.
- A Vitest unit test covers the mutator's error mapping and abort behaviour with a stubbed `fetch`.

## Implementation Notes
- The generated client is an implementation detail of `HttpFileManagerClient` (0012); feature code
  must never import from `api/generated/` directly.
- Keep the mutator free of Mithril imports so it is testable in isolation.

## Agent Notes
- 2025-06-12 claude: Implemented the Orval Fetch client and `api:check` wiring.
  - `frontend/src/api/fetch-mutator.ts` (new): custom Orval mutator owning base URL
    resolution (`VITE_API_BASE_URL`, overridable via `setBaseUrlOverride` for tests),
    JSON/empty-body handling (204/205/304), `AbortSignal` pass-through (the caller's
    `options.signal` is forwarded to `fetch` untouched), an optional pluggable session
    header (`setSessionHeaderProvider`), and a typed `ApiError` (`code`, `message`,
    `requestId`, `details`, plus `status`) built from non-2xx responses — falling back
    to a synthetic `unknownError` when the body isn't JSON or doesn't match the shape.
    Returns `{ status, data, headers }`, matching Orval 8.x's fetch-client response
    shape (confirmed by inspecting the generated output; older docs examples only
    show `{ status, data }`).
  - `frontend/src/api/fetch-mutator.test.ts` (new): 7 Vitest cases with a stubbed
    global `fetch` covering success parsing, base-URL override, session header
    attachment, JSON and non-JSON error mapping, and `AbortSignal` forwarding +
    rejection propagation (the raw `DOMException` is never wrapped in `ApiError`).
  - `frontend/orval.config.ts` (new): `client: 'fetch'`, `mode: 'split'`, output to
    `src/api/generated/` (client in `file-manager-api.ts`, DTOs under `models/`),
    `override.mutator` pointing at `fetchMutator`, and an `override.header` banner
    explaining the file is generated and how to regenerate it.
  - `frontend/src/api/generated/` (new, generated): produced by `pnpm api:generate`;
    carries a `README.md` stating it must not be hand-edited.
  - `frontend/src/vite-env.d.ts`: added `VITE_API_BASE_URL?: string` to `ImportMetaEnv`.
  - `biome.json`: excluded `frontend/src/api/generated/**` from lint/format.
  - `pnpm-workspace.yaml`: added `allowBuilds: { esbuild: true }` — required because
    Orval pulls in `esbuild`, and pnpm's default supply-chain policy blocks new
    build scripts until explicitly allow-listed (the older `package.json#pnpm` field
    location is no longer read by this pnpm version).
  - `scripts/scripts.test.mjs`: replaced the stale "fails until task 0010 lands" test
    with one that runs `generate-api.sh` against the checked-in output and asserts a
    byte-identical re-generation (mirrors the 0009 determinism-test pattern).
  - `.github/workflows/ci.yml`: added a `pnpm run api:check` step to the `frontend`
    job (after the build step) plus `Swatinem/rust-cache@v2`, since `api:check`
    transitively runs `cargo run -p fm-server -- export-openapi`.
  - Verified: `pnpm --dir frontend run typecheck` clean; `pnpm --dir frontend test`
    21/21 passing; `pnpm exec biome check` clean on all files this task touched;
    `node --test scripts/*.test.mjs` 28/28 passing; `pnpm run api:check` regenerates
    `frontend/openapi/openapi.json` byte-identical and the generated client
    deterministically (confirmed via `git diff --exit-code` on tracked files —
    the only diffs surfaced were this task's own not-yet-committed source edits).
  - Not exercised: the new CI `api:check` step itself has not run in GitHub Actions
    (no CI access from this environment); local verification stands in for it.
