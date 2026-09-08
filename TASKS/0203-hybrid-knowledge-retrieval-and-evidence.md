# 0203 Hybrid knowledge retrieval and evidence

Status: done
Priority: high
Subsystem: semantic, search
Depends on: 0202

## Context

Build a low-level retrieval capability that treats native FTS and dense vector search as peers,
fuses ranks deterministically, diversifies by document, expands adjacent context after ranking, and
returns source-oriented evidence independent of RAG answer generation.

## Acceptance Criteria

- Support bounded `hybrid`, `fullText`, and `semantic` routes with explicit capability/fallback
  metadata.
- Fuse FTS and vector rankings with deterministic RRF; deduplicate by stable chunk/occurrence
  identity and never combine raw score domains.
- Track search route, retrieval reason, source query, ranks, final rank, file/chunk identity, and
  structural provenance in a privacy-safe optional trace.
- Enforce candidate/final-result limits, max results per file, complete-chunk token budgets, and
  post-rank adjacent/section context expansion.
- Preserve tenant/library/root/workspace authorization and stale/unavailable-source behavior.
- Cover FTS-only fallback, vector-only requests, hybrid fusion stability, diversity, scope
  isolation, cancellation, and restart.

## Implementation Notes

- Keep logical planning independent from physical Zvec MultiQuery batching.
- Evidence is a search result model first; LLM citations may adapt it later.

## Agent Notes

- 2026-09-08 Copilot: Created from restored Structured Knowledge Query phases 3 and 43-47.
- 2026-09-08 Copilot: Added `fm-semantic-worker::knowledge_retrieval`, a search-first capability
  that treats native FTS and dense vectors as peers. `FullTextCandidateIndex` is the lexical peer of
  `SemanticCandidateIndex` and deliberately returns rank order only, so BM25 relevance cannot leave
  the index and be combined arithmetically with cosine similarity; `ZvecStorage` implements both.
  `hybrid`, `fullText`, and `semantic` routes report capabilities (full text, query embeddings)
  independently and always publish requested/applied routes with a typed fallback reason. Hybrid
  degrades explicitly to FTS when embeddings are missing, fail, or the dense route errors, and to
  dense when no FTS index exists; `semantic` and `fullText` return typed unavailability instead of
  an empty page. Fusion is deterministic RRF over ranks with in-list deduplication, computed in
  Procyon rather than through Zvec `MultiQuery`, so logical planning stays independently testable.
  Ranking precedes stable-identity chunk deduplication (duplicate occurrences collapse into
  `duplicateSourceIds`), per-file diversity, final-result limits, the complete-chunk token budget,
  and only then bounded adjacent/section context expansion, so context can never change a rank.
  Every candidate is reauthorized against one SQLite publication snapshot under the tenant, library,
  root, and workspace filters, and evidence reports stale and unavailable sources rather than
  hiding them. The optional trace carries route, fallback reason, retrieval reason, source query,
  per-route ranks, fused score, final rank, file/chunk identity, and structural provenance, but no
  excerpt, content, heading, or filesystem path.
- 2026-09-08 Copilot: Verified 21 knowledge-retrieval tests covering hybrid fusion stability under
  reordered execution, FTS-only fallback, vector-only requests, capability/route errors, diversity,
  candidate/result/token limits, post-rank and section-bounded adjacency, tenant/root/workspace
  isolation, stale/unavailable reporting, privacy-safe tracing, typed cancellation, and identical
  results after a worker restart — including one test that drives a real native Zvec collection
  through both the FTS and vector routes. Also verified 72 worker library tests, the complete
  `fm-semantic-worker` suite (54 further integration tests, one pre-existing ignored), the 18
  feature-gated native Zvec tests, `cargo fmt --all --check`, and warning-free clippy with and
  without the `zvec` feature.
- 2026-09-08 Copilot: Post-review hardening removed the 1,024-candidate authorization truncation:
  fused candidates are now authorized in bounded ranked batches until exhausted, while selection
  remains result- and token-bounded and later duplicate occurrences are still recorded. A
  `CatalogReader` now owns one SQLite read transaction, so primary authorization and adjacency use
  the same publication snapshot. Cancellation is checked between authorization batches, selected
  primaries, adjacency reads, and final return. Regression coverage includes a visible candidate
  after the first 1,024 unauthorized IDs, duplicates beyond the result boundary, materialization
  cancellation, and publication during a retained read snapshot. Reverified all 73 worker library
  tests, all 92 `zvec`-enabled library tests, and warning-free clippy in both configurations.
