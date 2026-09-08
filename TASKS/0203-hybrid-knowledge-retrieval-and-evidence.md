# 0203 Hybrid knowledge retrieval and evidence

Status: open
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
