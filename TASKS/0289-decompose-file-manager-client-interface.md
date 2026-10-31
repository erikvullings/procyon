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
