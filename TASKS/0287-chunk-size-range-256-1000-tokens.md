# 0287 Chunk size range 256-1000 estimated tokens

Status: open
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
