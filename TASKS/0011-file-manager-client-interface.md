# 0011 FileManagerClient interface and runtime selection

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: frontend
Depends on: 0010

## Context
`file-manager-coding-agent-spec.md` §12 and §33 step 3. The frontend must be transport-neutral:
components depend only on `FileManagerClient`, never on `fetch`, `EventSource` or Tauri APIs
(§3 rule 1, §35).

## Acceptance Criteria
- `frontend/src/api/client/file-manager-client.ts` declares the interface from §12, with every
  method accepting an optional `AbortSignal` and `subscribe(listener)` returning `Promise<Unsubscribe>`.
- Methods not implemented in the current milestone are declared but may throw a typed
  `NotImplementedError` in the adapters that do not support them yet.
- `frontend/src/api/client/create-client.ts` selects the implementation from
  `VITE_RUNTIME` (`http` | `tauri` | `mock`) with an `assertNever` default (§12).
- Client selection happens in exactly one bootstrap location; a lint rule or test asserts no other
  module imports the concrete adapters.
- Frontend-facing model types live in `frontend/src/models/` and are re-exported from the generated
  DTOs where they match, so features never import `api/generated/` directly.
- Vitest test asserts `createFileManagerClient` returns the right adapter per runtime value and
  throws on an unknown value.

## Implementation Notes
- Do not scatter Tauri runtime checks through UI components (§12).
- Keep the interface aligned with the backend's semantic operations, not HTTP concepts (§7).

## Agent Notes
- 2026-07-29 claude: Implemented the `FileManagerClient` interface, runtime selection and the
  frontend model layer.
  - `frontend/src/api/client/file-manager-client.ts` (new): the §12 interface verbatim, every
    method taking an optional `AbortSignal`, `subscribe` returning `Promise<Unsubscribe>`, plus
    the typed `NotImplementedError` (`"<Method> is not implemented until task <n>; see
    TASKS/<n>-*.md"`, mirroring `scripts/not-implemented.sh`'s wording).
  - `frontend/src/api/client/{http,mock,tauri}-file-manager-client.ts` (new): minimal adapter
    classes implementing `FileManagerClient`; every method throws `NotImplementedError` naming
    the task that owns its real implementation (0012, 0013, 0015 respectively) via a private
    `notImplemented()` helper, since those tasks own the DTO-mapping/fixture/`invoke` work.
  - `frontend/src/api/client/create-client.ts` (new): `createFileManagerClient(runtime)` switches
    on `RuntimeKind` (from `utilities/runtime.ts`, task 0009) and falls through to `assertNever`
    on an unreachable case — this is the single bootstrap location for adapter selection.
  - `frontend/src/api/client/create-client.test.ts` (new): 4 Vitest cases — correct adapter
    instance per runtime value, and a thrown error for a runtime value forced past the type
    system.
  - `frontend/src/api/import-boundaries.test.ts` (new): 2 Vitest cases that walk `frontend/src`
    and assert (a) only `create-client.ts` imports the three concrete adapters (non-test files),
    and (b) only `src/api/**` and `src/models/**` import from `api/generated` — the mechanical
    check the acceptance criteria ask for, since this repo uses Biome rather than an
    import-restriction ESLint rule.
  - `frontend/src/models/` (new): the frontend-facing model layer — `ids.ts`, `location.ts`,
    `entry.ts`, `workspace.ts`, `snapshot.ts`, `operation.ts`, `action.ts`, `plugin.ts`,
    `events.ts`, `requests.ts`, `runtime-capabilities.ts`, `index.ts` barrel. Types with a real
    backend DTO already (`Location`, `EntrySummary`, `EntryMetadata`, `DirectorySnapshot`,
    `LoadingState`, `Workspace` and its nested types, `ListDirectoryRequest`, `NavigateRequest`,
    `EntryMetadataRequest`) mirror `fm-transport-dto`'s camelCase DTOs field-for-field;
    `RuntimeCapabilities` is re-exported directly from the generated `RuntimeCapabilitiesDto`
    (task 0010) rather than duplicated. Types with no backend DTO yet (`Operation`,
    `ActionDescriptor`, `PluginDescriptor`, `BackendEvent`, `StartOperationRequest`,
    `ResolveConflictRequest`, `InvokeActionRequest`) mirror the domain shapes given in spec §17/§18
    where the spec defines them, and are explicitly commented as provisional (opaque
    `Record<string, unknown>`/`unknown` fields) where the spec leaves the shape open, naming the
    task (0014, 0037+, 0052, 0053) that will settle it.
  - Removed `frontend/src/models/.gitkeep`, superseded by real content.
  - Not touched: `frontend/src/main.ts` / `AppShell` — nothing consumes a `FileManagerClient` yet,
    so wiring `createFileManagerClient` into the bootstrap would add an unused value; the app
    shell already threads `RuntimeKind` through from task 0009, and the actual client is wired
    in by the first consuming task.
  - Verified: `pnpm --dir frontend run typecheck` clean; `pnpm --dir frontend test` 27/27 passing
    (21 pre-existing + 6 new: 4 in `create-client.test.ts`, 2 in `import-boundaries.test.ts`);
    `pnpm exec biome check frontend/src/models frontend/src/api` clean, no fixes needed after one
    autofix pass for line-wrapping. `git status --porcelain` shows only the files listed above
    changed (plus two pre-existing, unrelated `scripts/ci-workflow.test.mjs` Biome findings from
    before this task, left untouched).
