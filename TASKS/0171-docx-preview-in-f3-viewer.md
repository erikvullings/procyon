# 0171 DOCX preview in the F3 viewer

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0088

## Context

The F3 viewer can render text, Markdown, PDF, EPUB, comics, images, and media, but `.docx` files
still fall through to binary-text refusal. A content-oriented Word preview would cover the common
need to read a document without trying to reproduce Word's page-layout engine.

Use a native Rust DOCX reader to convert the package into semantic HTML or an intermediate document
model. `ferrodoc`/`ferrodoc-docx` is the preferred first prototype because it produces a
Pandoc-compatible AST and supports headings, formatting, lists, tables, links, images, footnotes,
metadata, and bookmarks. Render semantic HTML directly rather than converting through Markdown,
which would discard useful structure before the existing sanitized-HTML viewer sees it.

## Acceptance Criteria

- F3 recognizes `.docx` and opens a read-only, content-oriented in-app preview.
- DOCX parsing lives behind a provider-neutral `fm-application` capability and consumes VFS data;
  frontend code never receives a native path or calls host-specific APIs.
- The rendered result preserves, where supported by the selected parser, headings, paragraphs,
  inline emphasis, lists, tables, links, footnotes, and embedded images.
- Generated HTML is sanitized with the existing DOMPurify policy before insertion, following the
  EPUB/Markdown preview precedent. External links use safe target/rel attributes and package
  content cannot execute scripts or load arbitrary external resources.
- Parsing has explicit limits for source bytes, expanded ZIP bytes, ZIP entry count, XML depth,
  image count, image bytes, and rendered output. Files outside the budget receive a clear external-
  application fallback rather than an unbounded allocation.
- Cancellation, source-revision invalidation, session cleanup, and browser/Tauri behavior match the
  existing viewer contracts.
- The preview clearly remains content-oriented: unsupported Word layout features such as exact
  pagination, floating objects, text boxes, charts, headers/footers, tracked changes, and field
  evaluation are omitted or represented honestly rather than approximated invisibly.
- Existing F3 search, text selection, copy, metadata, and external-open actions work where meaningful.
- Tests cover representative formatting, tables, links, embedded images, malformed OOXML, ZIP-bomb
  limits, sanitization, cancellation, source changes, fallback behavior, and HTTP/Tauri/mock parity.

## Implementation Notes

- Prototype `ferrodoc` with default features disabled and only the DOCX/HTML pieces enabled; verify
  its actual output and binary-size impact before committing to it. Keep the converter behind an
  application-owned interface so the parser can be replaced without changing transport or UI.
- Reuse the sanitized chapter/content rendering path in
  `frontend/src/features/preview/epub-preview.ts` and `file-viewer.ts` rather than creating a second
  arbitrary-HTML renderer.
- Do not use `docx-rs`, whose primary focus is writing documents. Lower-level `docx` or `ooxml`
  parsing is a fallback only if the `ferrodoc` prototype exposes a blocking correctness issue.
- Prefer bounded package resources referenced by IDs over one enormous HTML response containing
  base64 images. Any new session API should remain useful for remote and sequential VFS providers.

## Agent Notes

- 2026-08-29: Created from the Office-preview design discussion. The recommended scope is semantic
  document reading, not Word-compatible layout. `ferrodoc` 0.7 is the leading native Rust candidate;
  HTML is preferred over a DOCX-to-Markdown-to-HTML pipeline to preserve tables and images.
