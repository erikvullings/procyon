# 0182 Incremental semantic ingestion and reconciliation

Status: done
Priority: high
Subsystem: backend, search, events
Depends on: 0179, 0180, 0181

## Context

An enrolled library must remain current without repeatedly re-embedding whole documents or losing
hours of work after shutdown. Filesystem events provide low latency but can be dropped, coalesced,
or missed while Procyon is closed; timestamps alone are unreliable. Reconciliation and content
hashes must establish truth.

The pipeline should publish complete document generations, reuse globally cached vectors for
unchanged chunk fingerprints, and isolate individual failures. Procyon stops the worker when the
application closes rather than installing an always-running login service.

## Acceptance Criteria

- A persisted job/catalog state machine covers discovered, hashing, converting, chunking,
  embedding, staging, publishing, deleting, complete, failed, paused, and cancelled work with an
  explicit in-progress stage and bounded attempt metadata.
- Local filesystem events enqueue near-immediate candidate updates. Every worker start reconciles
  enrolled roots, and running workers reconcile at a configurable default 30-minute interval.
  Manual Refresh is available.
- Cheap file identity, size, and modification metadata select hash candidates; streamed content
  hashes decide whether bytes changed. Metadata-only moves/renames update occurrences without
  re-embedding content.
- Changed documents are converted and chunked into a staging generation. Fingerprint matching and
  the global cache reuse unchanged vectors regardless of shifted positions; only cache misses are
  embedded. Removed occurrences decrement references and reclaim unreferenced vectors/artifacts.
- The previous complete generation remains searchable and visibly stale/updating until all new
  catalog, extracted-text, vector, provenance, and summary prerequisites publish atomically. A
  source opened from stale evidence warns when the indexed content hash differs.
- A crash/restart replays idempotent stages without duplicate visible chunks, skipped work, mixed
  generations, or lost deletion requests. Queue/progress state survives normal shutdown.
- One failed document retries transient errors with bounded backoff, then records stage/reason and
  leaves the rest of the root progressing. Retry/Skip controls exist; changed content or relevant
  converter/model versions make it eligible again.
- A complete successful listing is required before declaring unseen entries deleted. Missing or
  offline roots become unavailable without removing indexed evidence.
- Eco/Balanced/Fast profiles bound CPU, memory, I/O, and concurrency. Interactive queries preempt
  ingestion; low battery, thermal pressure, configured free-space reserve, and low disk pause safely
  with an actionable reason.
- Progress and coverage events flow through the existing event model with HTTP/Tauri parity and
  contain counts/stages/errors but no source excerpts or queries.
- Tests cover event coalescing, missed-event reconciliation, timestamp lies, metadata-only moves,
  localized edits, cache reuse, deletion/reference counts, offline roots, partial listings, crash at
  every stage boundary, cancellation, retry exhaustion, resource profiles, low disk, and query
  preemption.

## Implementation Notes

- Reuse task 0020's watcher/delta concepts where they fit, but do not let a watch registration block
  navigation or ingestion responses (see 0156).
- The catalog is the source of truth. Zvec has no cross-store transaction, so use staged generations
  and idempotent recovery rather than claiming a distributed transaction.
- Summary generation is a separate optional phase in 0185; base chunks must be searchable without
  an LLM profile or summary.

## Agent Notes

- 2026-09-04: Split from 0176. The required freshness policy is watch + startup/30-minute
  reconciliation, content-hash verification, and chunk-level vector reuse.
- 2026-09-05: Implemented the worker-owned persisted state machine and convert/chunk/cache/embed/
  stage/publish pipeline. SQLite remains visibility authority and Zvec remains derived; failed
  publication leaves the prior complete generation searchable, retries reuse staging/cache state,
  and occurrence deletion removes authoritative references before idempotent derived cleanup.
- 2026-09-05: Added an injectable IPC ingestion backend with durable scoped job registration while
  retaining the deterministic in-memory worker constructor. Added bounded retry backoff and
  exhaustion, explicit failed-job Skip, cancellation, disk/battery/thermal pauses, and interactive
  query priority.
- 2026-09-05: Added host-side provider-event coalescing, content-hash truth, metadata-only move
  planning, complete/partial/offline listing semantics, startup/periodic/manual scheduling, stale
  evidence assessment, and shared path-free progress/coverage events with SSE/Tauri parity.
- 2026-09-05: Tests cover localized cache reuse, retry exhaustion and Skip, deletion, resource
  pauses, cancellation, scope isolation, timestamp lies, missed-event reconciliation, moves,
  partial/offline roots, startup/manual cadence, and replay from every persisted stage. Concrete
  model activation and production index thresholds remain intentionally gated by task 0188.
- 2026-09-06: Desktop startup now reconciles every enrolled root, including roots whose initial
  enrolment occurred while the worker was unavailable. Developer startup resolves worker/runtime
  paths from durable installed-component state rather than a subsequently rebuilt bundle catalog.
  Failed conversion jobs and oversized documents are isolated per document, allowing the complete
  root listing to commit while reporting failed and skipped occurrences. Developer index storage
  now uses the official Zvec category counted by Settings.
