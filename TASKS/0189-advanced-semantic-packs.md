# 0189 Advanced converters, acceleration and reranking

Status: done
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
- 2026-09-05: Added baseline-first isolated advanced conversion with bounded byte-only input,
  provenance precision, typed omissions, cancellation/time limits, malformed-output rejection, and
  safe removal. Added CPU-fallback acceleration with parity proof or explicit model-space migration,
  a bounded quality-gated local reranker with measured cost, and a separately named deterministic
  Hybrid mode using weighted reciprocal-rank fusion after filters and tenant/library enforcement.
- 2026-09-05: Added independently signed converter/acceleration/reranker manifests with target,
  protocol and index compatibility, checksums, resource disclosures, administrator policy,
  evaluation measurements for all nine required fixture dimensions, one-version rollback, and
  isolated family removal. Focused suites passed 261 tests; workspace validation passed 2,199 Rust
  tests (5 skipped), 1,777 frontend tests, 41 script tests, full lint, and the production build.
  Concrete optional pack binaries are intentionally not selected by this product-boundary task;
  real GPU/driver execution and Windows/Linux artifact smoke tests remain release checks for each
  future pack.
