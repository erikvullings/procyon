# 0012 HTTP FileManagerClient adapter

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: frontend
Depends on: 0011

## Context
`file-manager-coding-agent-spec.md` §12 and §33 step 3. Wraps the Orval-generated client behind the
transport-neutral interface.

## Acceptance Criteria
- `frontend/src/api/client/http-file-manager-client.ts` implements `FileManagerClient` using only
  the generated client plus the fetch mutator.
- All calls forward `AbortSignal`; superseded requests are aborted by the caller, not swallowed.
- Errors surface as the shared typed `ApiError`; raw `Response` objects never escape the adapter.
- `subscribe()` delegates to the SSE event stream (implemented in 0033); until then it returns a
  no-op unsubscribe and is covered by a TODO referencing 0033.
- Vitest tests with a stubbed generated client cover: happy path mapping, error mapping,
  cancellation propagation.

## Implementation Notes
- Base URL comes from the mutator (dev uses the Vite `/api` proxy).
- Keep DTO → model mapping in one place so the Tauri and mock adapters can reuse it.

## Agent Notes
- Not started.
- 2026-07-29 claude: Implemented `HttpFileManagerClient` against the generated client as it exists
  today. The backend OpenAPI document (`frontend/openapi/openapi.json`) currently only defines
  `GET /api/v1/health` and `GET /api/v1/runtime`, so `getRuntimeCapabilities` is the only method
  with a real generated call to wrap; every other interface method still has no backing endpoint
  or Orval-generated function, so — per 0011's own design (`NotImplementedError` for
  not-yet-supported methods) — they continue to throw it, now naming the concrete backend task that
  will add their endpoint instead of pointing back at this task:
  - `navigatePane` / `listDirectory` / `getEntryMetadata` → 0019 (adds
    `POST /api/v1/directories/list`, `POST /api/v1/navigation/open`, `POST /api/v1/entries/metadata`).
  - `startOperation` / `cancelOperation` / `resolveConflict` → 0036 (adds the operations API).
  - `listActions` / `invokeAction` → 0049 (backend action registry).
  - `listPlugins` → 0053 (plugin manifest and discovery).
  - `getWorkspace` — **flagged gap, not guessed at**: spec §8 lists
    `GET /api/v1/workspaces/{workspaceId}`, but no `TASKS/*.md` currently claims implementing it;
    left throwing `NotImplementedError('HttpFileManagerClient.getWorkspace', 'TBD')` with a comment
    explaining the gap rather than inventing an owning task number.
  - `frontend/src/api/client/http-file-manager-client.ts`: `getRuntimeCapabilities` forwards the
    caller's `AbortSignal` into the generated call's `RequestInit`; the response's `.data` is
    returned directly since `models/runtime-capabilities.ts` already re-exports
    `RuntimeCapabilitiesDto` verbatim, so there is no DTO → model transform to centralise yet (noted
    in a class-level comment for whoever adds the next real mapping, per the Implementation Notes).
    Errors are not re-wrapped: `fetchMutator` (task 0010) already guarantees non-2xx responses
    become a typed `ApiError` and that raw `Response` objects never escape it, and abort rejections
    already propagate unwrapped (matching `fetch-mutator.test.ts`'s existing behaviour) — the
    adapter simply forwards both, so cancellation is never swallowed.
  - `subscribe()` no longer throws: it returns a no-op `Unsubscribe` behind a
    `// TODO(0033): delegate to the SSE event stream` comment, per the acceptance criteria.
  - `frontend/src/api/client/http-file-manager-client.test.ts` (new): 7 Vitest cases — happy-path
    mapping of `getRuntimeCapabilities`, `AbortSignal` forwarding into the generated call, an
    `ApiError` rejection propagating unchanged, an abort `DOMException` rejection propagating
    unchanged, `subscribe()` resolving to a callable no-op, and `NotImplementedError` for
    `navigatePane`/`startOperation` naming the correct owning tasks (0019/0036).
  - Verified: `pnpm --dir frontend exec vitest run src/api/client/http-file-manager-client.test.ts`
    7/7 passing; `pnpm --dir frontend exec vitest run src` 36/36 passing (27 pre-existing + this
    task's 7, plus 2 pre-existing untracked test files noted below); `pnpm --dir frontend run
    typecheck` clean for every file this task touched (see known gap below); `pnpm exec biome check`
    clean on both new/changed files after one autofix pass for line-wrapping.
  - **Known gap / not this task's scope**: the working tree already had unrelated untracked files
    before this task started — `frontend/src/api/client/mock-file-manager-client.ts`,
    `mock-file-manager-client.test.ts`, `mock-directory-generator.ts`,
    `mock-directory-generator.test.ts`, and a top-level `fixtures/` directory (apparent
    in-progress work for task 0013). `mock-file-manager-client.test.ts` fails
    `pnpm --dir frontend run typecheck` (references a non-existent `MockClientError` export) even
    though it passes at runtime under Vitest's transform-only execution. Left entirely untouched
    and excluded from this commit — flagging here rather than silently leaving it, per this task's
    own working instructions.
