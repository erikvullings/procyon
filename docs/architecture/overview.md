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

The worker's semantic index has two deliberately separate authorities. The host-side
`fm-semantic-library` catalog above decides consent and which opaque records may enter the worker.
Inside the worker, SQLite is authoritative for index lifecycle: exact library manifests,
documents, occurrences, jobs, component revisions, cached-vector references, and complete versus
staging generations. Zvec contains only derived occurrence-level vectors and structured filter
fields. Queries use Zvec for candidates, then re-authorize those IDs against one SQLite read
snapshot, so publication is old-complete or new-complete and cached vectors never cross tenant
boundaries. Superseded records are reclaimed only after active readers drain. The official
`zvec-rust` dependency is optional and feature-gated because its build script downloads and
dynamically links a native library; see
[Zvec Rust SDK qualification](zvec-rust-sdk.md) for the pinned versions and packaging matrix.

Incremental ingestion preserves the same authority split. The host coalesces provider events and
performs startup, periodic (30-minute by default), or manual reconciliation; only a complete
listing can prove deletion, while partial and unavailable roots retain evidence. Streamed content
hashes, not provider timestamps, establish change. The worker persists each job stage in SQLite,
converts and structurally chunks bounded bytes, embeds only global-cache misses, stages a complete
generation, writes the derived index idempotently, and atomically changes SQLite visibility.
`WorkerServer::with_ingestion_backend` injects that durable pipeline without changing the
deterministic no-component worker used by tests and unavailable configurations. Progress and
coverage use the shared `fm-events` model, so browser SSE and Tauri carry the same path-free,
excerpt-free payloads.

Semantic retrieval extends the ordinary search lifecycle rather than creating an AI-specific
navigation stack. An explicit dense-only semantic predicate is embedded locally with the enrolled
library model, queried against Zvec, and re-authorized against the worker's SQLite snapshot.
Deterministic file-primary ranking selects the strongest extracted chunk and adds bounded section
diversity; generated summaries remain secondary evidence. The application materializes authorized
occurrences in the existing paged `search://` store and resolves opaque source IDs back through the
host-only semantic catalog, so filesystem locations never enter the worker. Search responses carry
bounded evidence, provenance, stale/availability state, and honest coverage. The frontend exposes
that evidence beside the ordinary pane, opens its excerpt in the existing viewer, and stores
explicit relevance judgements locally for user-triggered export only.

Structured Knowledge Search extends that retrieval boundary with native full-text search as a peer
to vectors. Its request and deterministic plan contain only subjects, knowledge needs, explicit
related terms, authorized scopes, and retrieval options. Zvec executes independent FTS and dense
routes and fuses ranks without mixing score domains; missing or incompatible query embeddings
degrade explicitly to FTS. Source evidence remains useful offline and without a generation profile.
Action and application context belong to a separate optional answer request and never affect
retrieval text unless repeated explicitly as a related term; a typed action may select conservative
default needs when the user selected none. `fm-application::knowledge` owns the canonical models
and pure planner, emits at most eight stable source searches, and reports expansions omitted by
bounds. See
[ADR 0012](../decisions/0012-structured-knowledge-search.md).

Generation is an optional application capability independent from semantic indexing. Named
OpenAI-compatible profiles are owned by `fm-application`, while `fm-credentials` retains their
tokens and settings retain only opaque credential references. The capability supports local server
presets and explicit cloud endpoints, normalizes bounded Chat Completions probes, and requires
host-bound informed consent before activating a cloud profile. Axum, Tauri, and the mock adapter
share the same transport-neutral client contract; server deployments deny loopback and all cloud
hosts by default unless `PROCYON_LLM_ALLOWED_HOSTS` explicitly allow-lists them.

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
