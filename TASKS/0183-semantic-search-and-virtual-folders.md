# 0183 Semantic search and virtual-folder integration

Status: open
Priority: high
Subsystem: frontend, backend, search
Depends on: 0162, 0166, 0182

## Context

Semantic retrieval should extend Procyon's existing search lifecycle and virtual locations rather
than become a separate AI screen. Users need predictable distinctions between literal filename,
content, semantic, and future hybrid ranking. A query must never implicitly enrol a folder.

Zvec returns chunk occurrences, while a file manager needs actionable file entries. Results should
therefore rank files from supporting chunks, preserve stable entry/navigation behaviour, and expose
the evidence without flooding the pane with duplicate rows.

## Acceptance Criteria

- The existing Find/Search experience offers explicit Name, Content, and Semantic modes. Semantic
  mode is dense-vector-only in this task; Hybrid remains a separately named future mode.
- Semantic search defaults to the current folder recursively and has a prominent Entire library
  scope. It supports enrolled roots, selected saved-search scope, type/MIME/date and other compatible
  structured filters without silently changing semantics.
- Query text is embedded locally with the library's exact model and sent to filtered Zvec retrieval.
  No LLM query rewrite, remote embedding, cross-encoder reranking, or hidden lexical fusion occurs.
- Chunk candidates are grouped into primary file results using a documented deterministic scoring
  and diversity rule. Each row shows the best excerpt and can expand bounded additional matching
  sections; exact duplicate files remain actionable occurrences but may be grouped to avoid crowding.
- Activating evidence uses the strongest real provenance: PDF page, DOCX heading/bookmark, PPTX
  slide, spreadsheet sheet/range, text/code line, then file-level fallback. Unavailable or stale
  sources open a retained indexed-evidence inspector with clear status.
- Result DTOs expose score, chunk kind, excerpt, provenance, indexed content hash/generation,
  availability, stale state, and coverage without leaking inaccessible tenant/library data.
- Unenrolled, pending, excluded, skipped, stale, unavailable, and failed portions of the requested
  scope produce honest partial-coverage status and counts. Search returns ready results immediately
  and offers a separate confirmed Include folder action; it never indexes on demand.
- Semantic queries use the existing cancellable/paged search lifecycle, events, virtual-location
  navigation, stable entry references, HTTP/Tauri parity, and mock fixtures.
- Saved searches/smart folders persist a versioned semantic predicate and visible scope/coverage
  semantics; they never persist executable worker commands or opaque model-specific query vectors.
- Users can mark results relevant/not relevant and optionally save local evaluation cases. Feedback
  remains local and export is explicit; no telemetry is introduced.
- Tests cover mode isolation, scope/filter enforcement, grouping/ranking, duplicates, paging,
  cancellation, partial coverage, stale/unavailable evidence, citation navigation, saved search
  serialization, tenant isolation, adapter parity, accessibility, and multilingual fixtures.

## Implementation Notes

- Extend the search virtual-location model from 0162 and source-mode reporting from 0166. Do not
  create a second result store or navigation stack.
- Keep summary chunks from 0185 discoverable but group them under their source document and label
  generated evidence. Summary text must not recursively dominate ordinary content retrieval.
- Record a retrieval-quality fixture baseline before changing the embedding model or chunking rules.

## Agent Notes

- 2026-09-04: Split from 0176. Semantic search is explicitly dense-only v1, file-primary with
  expandable chunks, current-folder scoped by default, and honest about partial indexing.