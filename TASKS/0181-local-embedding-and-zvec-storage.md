# 0181 Local embedding runtime and Zvec storage

Status: open
Priority: high
Subsystem: backend, search, storage
Depends on: 0177, 0180

## Context

Semantic search requires reproducible local embeddings and durable vector retrieval without a
separate database server. Zvec is the selected embedded store and now has an official Rust SDK, but
the installed project skill primarily documents Python/Node bindings. The Rust API, native library
packaging, schema evolution, filtering, iterator, crash, and single-writer behaviour must be proven
against the pinned version rather than assumed from other bindings.

One embedding model applies to a device-local library so scores are comparable. Occurrences and
provenance are document-specific, while identical normalized embedding inputs share cached vectors.

## Acceptance Criteria

- A spike pins and documents the official `zvec-rust` and native SDK versions, supported targets,
  Apache licensing/NOTICE obligations, static/dynamic linkage, deployment size, and cross-compilation
  requirements for macOS arm64/x64, Windows x64, and Linux x64/arm64.
- Tests verify the actual Rust API for collection create/open, schema fields, FP32 cosine vectors,
  FLAT/HNSW selection, scalar inverted filters, insert/upsert/delete, iteration, query limits,
  optimize/compaction, concurrent readers, single writer, abnormal shutdown, and reopen recovery.
- A CPU-first local embedding interface loads an immutable curated model revision, tokenizes and
  batches bounded inputs, normalizes vectors consistently, reports dimensions/model identity, and
  supports cancellation and resource profiles. It performs no remote requests.
- A library manifest binds Zvec schema version, dimensions, distance metric, model revision,
  tokenizer, converter/chunker versions, and normalization. Incompatibility returns a typed
  migration requirement and never opens an index under false assumptions.
- The SQLite semantic catalog is authoritative for libraries, documents, generations, occurrences,
  jobs, component versions, and reference counts. Zvec stores derived searchable chunk/summary
  records and filter fields, not lifecycle truth.
- Embedding cache keys include normalized embedding input, exact model revision, tokenizer-affecting
  settings, and chunker version. Identical chunks across edits/documents reuse one vector while each
  occurrence retains library/tenant, source, provenance, availability, and generation filters.
- Storage supports document-generation staging and atomic publication so queries see either the old
  complete generation or the new complete generation, never a mixture. Superseded unpinned records
  are reclaimed after in-flight readers finish.
- Query APIs accept tenant/library/root/workspace/type/date/concept/generation filters and enforce
  them in storage/retrieval, not only in the frontend. A query can return occurrence-level evidence
  without exposing another tenant's cached vector references.
- Index choice is measured rather than hard-coded: exact FLAT is permitted for small libraries and
  HNSW is the expected larger default; migrations/rebuilds preserve correctness across the chosen
  threshold.
- Tests cover deterministic embeddings, dimension mismatch, cache reuse/reference counting,
  duplicate occurrences, filtered retrieval, tenant isolation, staged publication, crash recovery,
  migration refusal, deletion, and Windows/macOS/Linux packaging checks.

## Implementation Notes

- Keep all Zvec access inside the worker's storage adapter. No frontend, HTTP handler, Tauri command,
  or general application service should depend directly on the SDK.
- Use structured scalar fields and Zvec filters; do not encode access scope or metadata into IDs or
  ad hoc query strings.
- Acceleration is optional future work in 0189. CPU output is the compatibility baseline; an
  accelerated runtime must prove vector parity before reusing an existing index.

## Agent Notes

- 2026-09-04: Split from 0176. Zvec is fixed as the store, but concrete SDK/API assumptions and the
  default embedding model remain evidence-driven decisions for this task.