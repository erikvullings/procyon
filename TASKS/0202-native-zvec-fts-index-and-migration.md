# 0202 Native Zvec full-text index and migration

Status: open
Priority: high
Subsystem: semantic, search, storage
Depends on: 0201

## Context

Knowledge Search requires lexical retrieval as a peer to vectors, especially for specialist terms,
acronyms, identifiers, document titles, and headings. Add native Zvec FTS to the existing derived
chunk index and migrate vector-only indexes without silently deleting authoritative or derived data.

## Acceptance Criteria

- FTS-index chunk content and, where supported safely, title/file/section metadata using a
  Unicode-capable configuration suitable for English, Dutch, and German or French.
- Expose bounded FTS-only queries independently from vector queries.
- Detect schema-v1/vector-only indexes and add the FTS index in place when supported; otherwise use
  the existing staged migration/rebuild and rollback machinery.
- Preserve insert/update/delete consistency across FTS and vectors.
- Keep FTS usable when embeddings or the compatible model runtime are unavailable.
- Cover exact terms, phrases, capitalization, Unicode, titles/headings, updates, deletions,
  interrupted migration, restart, and rollback.

## Implementation Notes

- Use zvec-rust 0.7.0's real API, not examples from another language binding or release.
- Never combine BM25 and cosine scores arithmetically.

## Agent Notes

- 2026-09-08 Copilot: Created from restored Structured Knowledge Query phases 1-2.
