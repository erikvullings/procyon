# 0190 Local semantic developer bundle

Status: done
Priority: high
Subsystem: desktop, semantic, packaging
Depends on: 0178, 0181, 0182, 0183

## Context

The semantic epic provides the worker protocol, managed-component lifecycle, ingestion pipeline,
vector storage, and search integration, but desktop development still injects only a lifecycle
simulator. A developer cannot install components, enrol a local folder, ingest documents, and run a
real semantic search through the application. Create an explicitly non-production bundle that
exercises that complete path without weakening the signed production-pack contract or implying that
its model is release-quality.

## Acceptance Criteria

- A repository command builds a host-platform developer bundle containing the real semantic worker,
  runtime/model metadata, checksums, and a local catalog in a deterministic output directory.
- The bundle is signed only by a public repository development key and is local-only by explicit
  policy: release builds reject it, arbitrary paths or URLs are not accepted from the frontend,
  and its catalog/model identity is visibly labelled as development-only.
- `pnpm dev:tauri` can opt into the bundle, install it through the existing managed-component
  lifecycle, start the packaged worker, and report actionable errors for a missing, stale, or
  incompatible bundle.
- The active developer capability connects the existing library enrolment, conversion, ingestion,
  embedding/vector storage, and semantic-search surfaces; it is not another UI-only lifecycle
  simulator.
- A deterministic end-to-end fixture proves install, local-root enrolment, ingestion, persisted
  index reopen, and a semantic query returning evidence from the enrolled document.
- Documentation explains how to build, run, share, and remove the developer bundle, its supported
  targets, and why it must not be published as a production component pack.
- Existing absent-component, browser/server authority, production signature, and ordinary
  file-manager behavior remain unchanged.

## Implementation Notes

- Reuse `fm-semantic-components`, `fm-semantic-worker`, and the task 0182 ingestion coordinator.
- Keep the normal production constructor inert. Gate developer loading behind debug assertions and
  an explicit environment variable.
- Prefer a small deterministic development embedding implementation already covered by the worker
  contracts; do not select or claim a production embedding model.
- Bundle creation may read repository-owned artifacts only and must emit checksums rather than
  bypassing component verification.

## Agent Notes

- 2026-09-05: Created after task 0178 intentionally deferred a production catalog/model. The user
  requested a concrete bundle for local end-to-end testing; this task owns that development-only
  milestone without relaxing the production release gate.
- 2026-09-05: Added `pnpm semantic:bundle:dev` and `pnpm dev:tauri:semantic`. The deterministic
  host-platform bundle includes the real worker, cataloged Zvec native runtime, explicit
  non-production model metadata, checksums, and a catalog signed by the public development key.
  Debug Tauri builds verify and install every artifact through `ComponentManager`; release builds
  reject the developer-bundle environment variable.
- 2026-09-05: Added the feature-gated worker assembly with a bounded 384-dimensional Unicode
  token-hashing embedder, authenticated on-demand launch, lazy tenant/library registration, real
  conversion and structural chunking, persistent SQLite catalog/Zvec storage, manifest validation
  on reopen, and relocatable native-library loading. The ordinary worker constructor remains inert
  and unchanged.
- 2026-09-05: Added explicit application reconciliation for enrolled roots. It enumerates through
  VFS with paging, recursion, size and cancellation bounds; applies curated eligibility; retains
  locations and workspace scopes only in the host catalog; streams bounded bytes to the worker;
  waits for ingestion; and commits a reconciliation generation only after a complete pass.
  Desktop developer enrolment triggers this pass without misreporting durable consent if indexing
  subsequently fails.
- 2026-09-05: Added bundle/catalog, recursive indexing and evidence resolution, cancellation,
  deterministic embedding, durable reopen, manifest mismatch, native loader, and desktop command
  regression coverage. The developer worker suite, focused application/component/desktop tests,
  full repository lint, release compilation, frontend component tests, and a real Apple-silicon
  bundle build passed. Generated API files remained unchanged.
