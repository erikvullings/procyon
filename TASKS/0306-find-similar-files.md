# 0306 Find similar files (semantic neighbors)

Status: open
Priority: medium
Subsystem: frontend, backend
Depends on: none

## Context
Procyon already computes embeddings (0181), ingests/reconciles them (0182), and does semantic
search + virtual folders (0183), with document centroids emerging from 0284. But there is no
"find files like this one" affordance — the single most natural way to *use* that investment.
Right now similarity is only reachable by typing a query.

## Acceptance Criteria
- A `core.findSimilar` action (backend action registry, 0049) on a single selected entry that
  returns the N nearest neighbours by embedding similarity, scoped to the active library/folder
  consent (0179) and honouring the same folder exclusions/revocation as semantic search.
- Frontend: context-menu entry + command-palette action; results open in a transient results view
  or are offered as a new smart-folder/saved-search (ties into 0162/0183).
- Distance metric and model profile are the ones that produced the source embeddings; a
  too-weak (token-hashing) profile degrades gracefully to exact/extension similarity or an honest
  "not enough semantic data here" message rather than noise.
- Works in both browser and Tauri hosts (spec parity); no new transport primitive.

## Implementation Notes
- Reuse the semantic search provider's nearest-neighbour path; the new piece is a *seed-by-entry*
  query rather than a text query. Prefer document-centroid vectors (0284) for whole-file similarity,
  falling back to chunk vectors for large/multi-topic files.
- Keep it opt-in and bounded (top-N, skip list) — never re-rank the entire library synchronously in
  the pane.
- Not a hard dependency on 0284 finishing: 0183's chunk embeddings are enough to ship v1.

## Agent Notes
- (none yet)
