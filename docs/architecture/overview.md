# Architecture overview

This document is prose, not a source of truth: the crate layering it describes is enforced
mechanically by [`crates/fm-test-support/src/architecture.rs`](../../crates/fm-test-support/src/architecture.rs)
and checked against the real `cargo metadata` graph in
`crates/fm-test-support/tests/workspace_architecture.rs`. If this document and that code ever
disagree, the code is correct — fix the prose, not the enforcement.

## Layering diagram (spec §3)

```text
┌───────────────────────────────────────────────────────────┐
│ Shared Mithril frontend                                   │
│                                                           │
│ Components, state, workspaces, pane views, dialogs         │
│                                                           │
│ Depends only on FileManagerClient                         │
└───────────────────────────┬───────────────────────────────┘
                            │
             ┌──────────────┴───────────────┐
             │                              │
┌────────────▼─────────────┐   ┌────────────▼──────────────┐
│ HTTP client adapter      │   │ Tauri client adapter      │
│                          │   │                           │
│ Generated REST client    │   │ Tauri invoke commands     │
│ EventSource/SSE           │   │ Tauri channels/events     │
└────────────┬─────────────┘   └────────────┬──────────────┘
             │                              │
             └──────────────┬───────────────┘
                            │
┌───────────────────────────▼───────────────────────────────┐
│ Rust application services                                │
│                                                           │
│ Navigation, workspaces, actions, operations, search,      │
│ metadata, plugins, settings and event publication         │
└───────────────────────────┬───────────────────────────────┘
                            │
┌───────────────────────────▼───────────────────────────────┐
│ Rust domain and engine                                    │
│                                                           │
│ VFS providers, operation scheduler, conflict resolution,  │
│ directory snapshots, filesystem watching and journaling   │
└───────────────────────────────────────────────────────────┘
```

The crate-level version of this same direction (domain → events / vfs traits / plugin API →
operations / providers / metadata / search → application services → Axum and Tauri hosts) is the
`CRATE_LAYERS` table in `architecture.rs` linked above; see that file for the authoritative,
per-crate layer assignment.

The optional semantic subsystem sits beside this normal startup path. `fm-application` exposes a
lazy `SemanticService` capability, `fm-semantic-protocol` owns its generated protobuf ABI, and
`fm-semantic-worker` supplies a per-user process reached only through owner-protected Unix-domain
sockets or Windows named pipes. The worker receives scoped opaque identifiers, metadata, and
bounded byte streams, never filesystem paths or provider access. If it is absent or incompatible,
the ordinary application-service path above remains available.

Optional worker, runtime, and model packages are managed separately by
`fm-semantic-components`. It verifies a signed catalog and artifact checksums, serializes lifecycle
mutations across processes, performs resumable atomic installs, retains one working worker for
rollback, and moves the semantic-data root through pause-copy-verify-switch. The application layer
owns authority and consent: desktop builds may receive an explicitly injected managed capability,
browser/server builds only report administrator-provisioned status, and normal construction remains
inert. A production catalog and concrete default model stay disabled until the evaluation task has
recorded retrieval quality, latency, licensing, package-size, and index-size measurements.

`fm-semantic-library` owns the provider-neutral enrolment policy and authoritative occurrence
catalog. Its settings-side policy stores stable root, library, and model identities without
credentials; catalog and runtime state remain beneath the configurable semantic-data root. Reads
and mutations share a cross-process lock, durable revision, and write-ahead journal spanning policy,
catalog, and pause state. Exclusions deny query and worker-feed scope immediately, then delete
occurrences, excerpts, summaries, labels, orphan vectors, and conversation pins through resumable
idempotent plans. Procyon enumerates VFS providers and supplies verified stable filesystem
identities; the semantic worker receives only approved opaque feed records and never follows paths.

`fm-semantic-conversion` is a pure parsing and chunking engine below the application layer. It
accepts bounded content streams and path-free trusted metadata, emits normalized structural units
with best-available provenance and explicit partial-result omissions, and returns typed outcomes for
unsupported, malformed, encrypted, scanned, cancelled, and over-budget input. Its versioned
structural chunker includes only bounded section hierarchy and source content in embedding input,
so moves and renames preserve reusable content fingerprints. `fm-application` supplies the narrow
provider-neutral VFS bridge; task 0182 owns ingestion scheduling and persistence.

## Mandatory rules (spec §3)

These ten rules govern every change to the frontend/backend boundary and the crate graph. They are
restated verbatim here so they can be found without opening the full specification; the
enforceable subset of them (layering, and the Axum/Tauri/anyhow dependency bans) is also asserted
by `architecture.rs`.

1. Frontend components must not call `fetch`, `EventSource` or Tauri APIs directly.
2. Axum handlers must remain thin.
3. Tauri commands must remain thin.
4. Core engine crates must not depend on Axum or Tauri.
5. Transport DTOs must not be reused indiscriminately as internal domain models.
6. Long-running operations must be represented as jobs.
7. The backend must own authoritative filesystem and operation state.
8. The frontend may hold presentation state, but must not implement file-copy semantics.
9. Browser and Tauri transports must provide equivalent application behaviour.
10. Platform differences must be represented through explicit capabilities.

Rules 1, 6, 8 and 9 are about where behaviour lives (frontend vs. backend, browser vs. Tauri) and
are reviewed by hand, since they concern intent rather than a dependency graph. Rules 2, 3 and 5 are
about handler/command thinness and DTO reuse, also reviewed by hand at the point handlers and
commands are added. Rule 4 and the `thiserror`/`anyhow` split from spec §2.2 are the two rules
`architecture.rs` checks mechanically today, via `HOST_ONLY_DEPENDENCIES` and
`APPLICATION_BOUNDARY_DEPENDENCIES`.

## Why this split

The dual-host requirement (browser + Tauri, spec §3 and ADR
[0001](../decisions/0001-browser-tauri-dual-host-architecture.md)) means the frontend cannot own any
transport-specific code: everything above the `FileManagerClient` boundary must work unmodified
against either adapter. Pushing operation state, filesystem truth and long-running work into the
Rust services (rules 6–9) is what keeps that boundary honest instead of becoming a leaky
abstraction that only browser or only Tauri actually satisfies.

## Related documents

- `docs/decisions/` — one ADR per architectural decision in spec §34.
- `docs/plugin-api/` — plugin API reference (task 0005 adds only a placeholder; filled in when the
  plugin runtime work lands).
- `docs/screenshots/` — UI screenshots referenced from the README and docs (placeholder for now).
