# 0287 Chunk size range 256-1000 estimated tokens

Status: in_progress
Priority: low
Subsystem: backend
Depends on: none

## Context
`ChunkerOptions` defaults (`crates/fm-semantic-conversion/src/chunk.rs`): target 400, max 512
estimated tokens, global for every embedder. 512 is the hard input limit of the shipped
`multilingual-e5-small`; the estimator is pessimistic (about 3 characters per token), so 400
estimated is roughly 1,200 characters. EmbeddingGemma accepts much more context (user: 8k) but 8k
would be far too large for retrieval. User suggestion (2026-10-29): aim for a minimum of ~256 and a
maximum of ~1,000 tokens; do not make it user-adjustable (settings complexity).

## Acceptance Criteria
- Chunk size limits derive from the active embedding model (never above its input limit), within the
  256-1000 token range, with no new user setting.
- Chunking identity version bumped so indexed documents rebuild.
- Retrieval measured before/after on the corpus (`~/Downloads/Custom Chat Leiden`) and the result
  recorded here; keep the old default if quality does not improve.
- Paragraph splitting rate reported before/after.

## Implementation Notes
- Units over `max_tokens` split at sentence boundaries; packing never crosses page or section
  boundaries. Related: 0285 (fragmented PDF units), PDF reference lists now rank as navigation.
- The range must respect the e5-small 512 limit: only models with larger context may exceed it.

## Agent Notes
- 2026-10-29: Created from the discussion on task 0285. Nothing implemented yet.
- 2026-10-29: Added `ChunkerOptions::for_model_input_limit(n)` (max = n clamped to 256..=1000 but never
  above n; target 25/32, overlap 3/32 of max; 512 reproduces the old defaults exactly). Tested. NOT wired
  into ingestion (`Chunker::default()` still used) and chunking version NOT bumped: no behaviour change
  shipped. Reason: no retrieval eval is feasible here (no query/gold set; `scripts/eval/pdf-gold` scores
  PDF conversion only), and chunk statistics alone do not show better retrieval.
  Measured on ~/Downloads/Custom Chat Leiden (21 of 24 PDFs have a text layer; 3 yield no document),
  text-layer converter, 23,663 units, via ignored test `fm-semantic-docling/tests/corpus_chunk_stats.rs`:
  | options | chunks | units split into parts | chunks <256 est | p10/p50/p90/max est |
  | 400/512 (current) | 2,366 | 14 (0.06% of units) | 601 | 83/350/397/510 |
  | 781/1000 | 1,353 | 0 | 316 | 13/666/774/781 |
  Chunks drop 43%; oversized-paragraph splitting is already negligible. To finish: build a retrieval eval
  (queries + expected passages) with the real embedder, compare, then wire
  `for_model_input_limit(identity.max_input_tokens)` in `fm-semantic-worker/src/ingestion.rs` and bump
  `STRUCTURAL_CHUNKER_VERSION` to 6 (embedding cache keys include it, so documents rebuild).
- 2026-10-29: Retrieval check with e5-small (`scripts/eval/retrieval`, 33 hand-written questions,
  corpus chunks from the region-based converter `.../baseline/12`; maximum in estimated tokens):
  256: MRR 0.380, hit@1 0.24; 384: 0.432 / 0.33; **512 (current): 0.492 / 0.42**; 768: 0.396 / 0.30
  (24 chunks over the 512-token model limit, truncated); 1000: 0.345 / 0.24 (242 truncated).
  Result: for e5-small keep 512 (smaller chunks lose, larger ones exceed the model). Since
  `for_model_input_limit(512)` equals the current defaults, wiring it changes nothing for e5.
  Not measured: any model above 512 (EmbeddingGemma weights are in `target/semantic-model-cache`
  but no runner exists here). Leave `for_model_input_limit` unwired and keep chunker version 5
  until a Gemma retrieval run shows larger chunks help. 33 questions is a small sample.
