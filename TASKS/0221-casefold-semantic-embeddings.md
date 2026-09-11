# 0221 Case-fold semantic embeddings

Status: done
Priority: high
Subsystem: semantic, search
Depends on: 0181, 0203

## Context

Semantic rankings currently depend on source/query casing even though file-manager users expect
`TRIZ`, `Triz`, and `triz` to retrieve the same evidence. Query-only normalization would move
queries into a different embedding space from indexed passages and make results less predictable.

## Acceptance Criteria

- Apply one versioned Unicode default case-folding transform to both document and query text before
  adding model role prefixes and generating embeddings.
- Preserve converted source text and excerpts unchanged for display, citation, and full-text search.
- Bind the preprocessing version into embedding cache and library-index compatibility identities.
- Force an automatic clean rebuild of existing embedding indexes when the preprocessing identity
  changes; never reinterpret existing vectors under the new policy.
- Add deterministic Unicode and mixed-case tests proving document/query vectors and semantic
  rankings are case-insensitive.
- Keep exact or quoted matching in the existing explicit full-text/hybrid route rather than
  allowing casing to silently perturb semantic rankings.
- Re-run exact-production evaluation because this intentionally changes the embedding space; do
  not lower the `0.84` absolute Ask floor to compensate.

## Implementation Notes

- Preserve the E5 `query:` and `passage:` prefixes after normalization.
- Treat the preprocessing version as release-candidate identity and migration evidence.

## Agent Notes

- 2026-09-11 Copilot: Created from the explicit requirement to make semantic retrieval
  case-insensitive while task 0220's exact-production evaluator was being implemented.
- 2026-09-11 Copilot: Added `unicode-default-case-fold/1` before both E5 passage and query role
  prefixes, bound it into cache/library/catalog identities, and forced clean replacement of the
  legacy case-sensitive derived index. The real pinned multilingual model produced identical
  vectors and rankings for case variants while preserving original excerpts.
