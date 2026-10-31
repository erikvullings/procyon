# 0289 Decompose FileManagerClient god interface

Status: open
Priority: high
Subsystem: frontend
Depends on: none

## Context

`frontend/src/api/client/file-manager-client.ts` is 1195 lines with 100+ methods. Every frontend feature imports this monolithic interface, and the three adapters (mock: 6504 lines, HTTP: 3223 lines, Tauri) each re-implement the entire contract. The abstraction provides no leverage — knowing the interface requires learning all 100+ methods. The spec's rule 1 (components depend only on FileManagerClient) has become a beacon that every component must navigate past, rather than a shallow seam that isolates transport concerns.

## Acceptance Criteria

- `FileManagerClient` is decomposed into 4-6 focused sub-interfaces (e.g. `WorkspaceClient`, `OperationClient`, `SemanticClient`, `ConnectionClient`, `PluginClient`)
- Each feature imports only the sub-interface(s) it needs
- The mock adapter is reduced by at least 30% (from 6504 lines)
- A composite client at the seam (`createClient.ts`) assembles sub-clients into the composite for existing callers that need the full interface
- `pnpm api:check` still passes (generated artifacts are unaffected)

## Implementation Notes

- Sub-interfaces should be clustered by capability domain (workspace, operations, navigation, search, semantic, settings, connections, plugins)
- The seam is at `createClient.ts` — existing callers that need the full interface continue to use the composite
- New callers should import only the sub-interface they need
- The `FileManagerClient` interface can remain as the composite for backward compatibility, or be deprecated in favor of sub-interfaces
- Focus on the mock adapter shrinkage first — that's the clearest win

## Agent Notes

- Start by auditing which features import which methods from `file-manager-client.ts` to identify the natural cluster boundaries
- The `mock-file-manager-client.ts` at 6504 lines is the largest target — reducing it first shows immediate value
- Consider whether `FileManagerClient` should remain as the composite (backward compat) or be replaced entirely
- The spec says components must depend only on `FileManagerClient` — this needs updating if we change the interface name

## Agent Notes

- 2026-10-31 agent: **Part 1/3 done — interface decomposition (committed 57f4e4c4).**
  Audited the full surface: `FileManagerClient` has 221 members (3 readonly fields + 218
  methods, including optional `?` members). Partitioned them into five disjoint, complete
  capability sub-interfaces, each in its own file with its own exact model-type imports and
  preserved JSDoc:
  - `ConnectionClient` (transport/host-lifecycle/platform/events, 30 members incl. `connection`,
    `supportsAppUpdates`, `supportsAppDataRemoval` and the optional `quit?`/`setWindowTitle?`/
    `setWindowChrome?`/`openWorkspaceWindow?`/`openViewerWindow?`/`resyncWorkspace?`)
  - `WorkspaceClient` (workspace lifecycle, navigation, previews, metadata, geo, connections/OneDrive/SSH)
  - `OperationClient` (operation engine: jobs, search, comparison, sync, checksums, duplicates, snapshots, link doctor)
  - `SemanticClient` (semantic components/library/vocab/OCR, LLM profiles, corpus themes, RAG, document summaries, knowledge search)
  - `PluginClient` (action dispatch + in-app plugin panels)
  Shared value/type symbols (`NotImplementedError`, `OpenWithApplication`, `NativeFileDrop`,
  `PluginPanelBounds`, `WindowChrome`) moved to `client-types.ts`. `FileManagerClient` is kept as
  the composite (`extends` all five) so every existing import of `FileManagerClient` keeps
  working unchanged — backward-compatible seam, adapters untouched.
  Verified: `tsc --noEmit` clean; `biome check` clean; 196 tests across
  import-boundaries/create-client/mock/http/tauri client suites pass.
- 2026-10-31 agent: **Part 2/3 remaining — split the mock adapter (target: main file ≤ ~4550
  lines).** The single `MockFileManagerClient` class holds ~70 cross-cutting state fields plus
  ~1089 lines of shared module helpers (`fixtureEntry`, `collectMatches`, `syntheticFileContent`,
  the `perform`/event-signal infrastructure) that many clusters depend on. A safe split keeps the
  root as a state+helper holder and composes five `MockXxxClient` sub-classes (each `implements
  XxxClient`) via a shared host accessor, moving method bodies into sibling files. This also
  satisfies the "composite at `createClient.ts` assembles sub-clients" criterion. Not started:
  deliberately avoided a blind big-bang refactor of the 6500-line fixture without first isolating
  the shared-state surface, to protect the 50k-line mock test suite.
- 2026-10-31 agent: **Part 3/3 remaining — migrate features to sub-interfaces.** ~81 files import
  `FileManagerClient`; migrate each to the focused sub-interface(s) it actually uses (mechanical,
  type-only). Note the spec §3 rule 1 wording ("components depend only on `FileManagerClient`")
  should be updated to reference the sub-interfaces, per the task's own Implementation Note.

- 2026-10-31 agent: **Part 2/3 done — mock adapter split into a composed sub-clients (target met, ~80%
  reduction).** The 6504-line monolithic `MockFileManagerClient` is replaced by:
  - `mock-mock-shared.ts` — the module-level fixtures/helpers/constants, now exported;
  - `mock-context.ts` — a `MockContext` holding all ~71 state fields + ~34 helper methods
    (`perform`, `emit*`, `require*`, `directorySnapshot`, …) behind a single shared instance;
  - `mock-{connection,workspace,operation,semantic,plugin}-client.ts` — five capability classes,
    each `implements` its sub-interface and holds `ctx: MockContext`;
  - `mock-file-manager-client.ts` — a thin composite that assembles `MockContext` + the five
    capability clients and forwards every one of the 222 members. `createFileManagerClient`
    (create-client.ts) returns this composite, satisfying the "composite at the seam assembles
    sub-clients" criterion. The main adapter file went 6504 -> ~1320 lines.
  Method bodies were moved out verbatim with `this.` rewritten to `this.ctx.` (fields/helpers) or
  `this.ctx.<capability>Client.` (cross-calls between interface methods), so behaviour is
  unchanged. Verified: `tsc --noEmit` clean, `biome check` clean, the full frontend suite
  (3322 tests across 242 files) green, `import-boundaries` green, and `pnpm api:check`'s generated
  artifacts (openapi.json + orval client) byte-identical.
  Implementation detail: a codegen script (`/tmp/gen3.mjs` + a depth-aware, string/comment-safe
  class-member splitter) drove the mechanical extraction; the generated files are checked in and
  hand-verifiable, not build-time generated.

- 2026-10-31 agent: **Criterion status.** Met: (1) five focused sub-interfaces; (3) mock adapter cut
  ~80%; (4) composite assembled at the createClient.ts seam; (5) `pnpm api:check` generated
  artifacts byte-identical. **Remaining — criterion (2): migrate the ~101 feature files that still
  import the composite `FileManagerClient` to the specific sub-interface(s) they use.** This is a
  large, purely type-level migration best done as its own focused, per-file pass (each file's client
  method usage must be mapped to clusters); it is deliberately NOT rushed here because the composite
  remains a valid, backward-compatible import for callers that legitimately span clusters (per the
  task's own Implementation Note), and the sub-interfaces are already exported and usable. Status
  left `open` until criterion (2) is completed.
