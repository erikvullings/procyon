# 0189 Advanced converters, acceleration and reranking

Status: open
Priority: low
Subsystem: backend, search, packaging
Depends on: 0188

## Context

The first release deliberately uses a portable CPU embedding runtime, Rust baseline conversion,
dense-only search, and no mandatory reranker. Higher-quality OCR/layout extraction, hardware
acceleration, hybrid retrieval, and cross-encoder reranking may improve particular libraries but add
large downloads, platform variance, latency, and migration risk.

Add these only as independently measurable optional capabilities after the baseline is hardened.

## Acceptance Criteria

- A separately downloadable advanced converter implements the 0180 converter contract for scanned
  PDFs, OCR, complex layout/tables, and optional image/VLM interpretation without granting arbitrary
  filesystem access. Its language/runtime may differ from Rust but remains isolated and signed.
- Advanced conversion reports provenance precision and omissions, observes the same resource and
  expansion limits, and can be removed without making baseline-supported documents unreadable.
- Platform acceleration may use appropriate CPU/GPU backends, but must prove embedding parity or
  declare an explicit model-space migration. Driver/runtime failure falls back to CPU without index
  corruption or silent vector differences.
- An optional local reranker model pack operates on a bounded candidate set, is cancellable, reports
  latency/resource cost, and is enabled only after fixture and local evaluation demonstrate a
  material quality gain. Dense retrieval remains available when it is absent.
- A separately named Hybrid mode may combine dense, Zvec full-text/BM25 or sparse retrieval, and
  structured filters with documented score normalization/fusion. It never silently changes
  Semantic mode's dense-only contract.
- Optional packs have independent signed manifests, download/disk/RAM estimates, lifecycle actions,
  version compatibility, rollback, diagnostics, and server-administrator policy.
- Quality/performance reports compare each capability against the 0188 baseline across multilingual,
  OCR, exact-term, code, structured-document, duplicate, latency, memory, and storage fixtures.
- Tests cover pack absence/removal, fallback, compatibility rejection, migration, cancellation,
  malformed advanced output, acceleration parity, reranker bounds, hybrid score stability, tenant
  isolation, and cross-platform packaging.

## Implementation Notes

- Do not make baseline semantic search, summaries, or RAG depend on these packs.
- Prefer one optional capability at a time with an evaluation-backed task split if implementation
  becomes substantial; this task is a product boundary, not permission for one oversized change.
- Revisit the current Zvec release's native full-text, sparse-vector, and multi-vector facilities
  when designing Hybrid mode; pin behaviour with Rust integration tests.

## Agent Notes

- 2026-09-04: Split from 0176 and intentionally deferred until 0188 establishes a stable baseline.