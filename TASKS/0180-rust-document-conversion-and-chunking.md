# 0180 Rust document conversion and structural chunking

Status: done
Priority: high
Subsystem: backend, preview, search
Depends on: 0171, 0172, 0173, 0179

## Context

Semantic retrieval quality, incremental vector reuse, excerpts, and citations all depend on one
bounded document representation. Procyon already has provider-neutral DOCX preview parsing, PPTX
rendering, spreadsheet viewing, and raw VFS streaming, but no shared semantic converter or
structure-aware chunk model.

Build a pure-Rust baseline first. High-fidelity OCR, scanned-PDF layout, and VLM interpretation are
optional advanced converters and must not block useful local semantic search.

## Acceptance Criteria

- A versioned converter interface accepts a bounded content stream plus trusted metadata and emits
  normalized structural units, warnings/omissions, and provenance without opening source paths.
- Baseline converters cover UTF text and common encodings, source code, Markdown, bounded HTML,
  DOCX, PPTX, XLSX/CSV, and text-layer PDF. Unsupported, encrypted, scanned, malformed, or
  over-budget files yield typed visible outcomes rather than fabricated or partial-success silence.
- Reuse or extract the proven bounded parsing surfaces from tasks 0171-0173 where practical; do not
  make preview-session DTOs the semantic storage model.
- Structural units retain headings/section paths, paragraphs, PDF pages/blocks, slides, spreadsheet
  sheets/table ranges, and code symbols/line ranges when available. Converter output never invents
  precision it cannot prove.
- A deterministic versioned chunker packs adjacent structural units to a token target, respects a
  maximum, and uses limited overlap only when splitting an oversized unit. It never crosses
  incompatible top-level boundaries merely to fill a target.
- Each chunk records normalized embedding input, display excerpt text, format kind, source order,
  exact/best-available provenance, and a stable fingerprint derived from embedding input plus
  converter/chunker versions.
- Embedding input is section hierarchy plus actual chunk/table content. Filename, absolute or
  relative path, generic document brief, generated description, and SKOS labels are not appended.
  A move or rename therefore does not force content re-embedding.
- Text sanitization removes only specified invisible/control hazards while preserving source
  position accounting. Visible instruction-shaped text is retained as untrusted evidence and may
  be flagged, never silently rewritten as a security measure.
- Per-format source, expansion, item-count, nesting, time, and output budgets prevent archive bombs
  or one document monopolizing the worker. Cancellation checkpoints exist throughout conversion.
- Fixture tests cover all baseline formats, deterministic output, local edit stability, provenance,
  encoding, malformed/encrypted/oversized inputs, cancellation, sanitization, and embedding-input
  exclusions.

## Implementation Notes

- Keep normalized source text suitable for excerpts and later BM25/hybrid retrieval, but do not add
  hybrid search in this task.
- Define the advanced converter as the same protocol capability, not a branch throughout ingestion.
  OCR/layout packs belong to 0189.
- Content fingerprints should permit global vector reuse while occurrence IDs and provenance remain
  document-specific.

## Agent Notes

- 2026-09-04: Split from 0176. The agreed quality boundary is a useful Rust baseline plus a future
  optional advanced pack; Docling-class dependencies are not required for initial semantic search.
- 2026-09-05: Implemented the pure `fm-semantic-conversion` engine and the application-layer VFS
  bridge. The versioned baseline converts bounded text, source, Markdown, HTML, DOCX, PPTX,
  XLSX/CSV, and text-layer PDF input into sanitized structural units with typed failures,
  omissions, cancellation, and best-available provenance. Added deterministic structural chunking
  with hierarchy-aware hard token limits, bounded overlap, display excerpts, and versioned BLAKE3
  fingerprints that exclude path metadata. Verified 114 crate tests, 6 application bridge tests,
  the workspace architecture test, and the full Rust/Biome lint gate.
- Fixed PDF-style heading orphaning when its first paragraph crosses the chunk target or needs
  splitting under the hard maximum. Same-page headings remain associated with their first text
  window and retain source order/provenance; section and page boundaries remain respected.
  The chunker pipeline identity advances to `structural/4`: existing libraries with a
  `structural/3` manifest require an explicitly authorized migration/reindex; this change does
  not touch user data automatically. A new signed worker candidate must reflect the new identity.
  `S1`/`S2` passage labels are summary evidence identifiers, not PDF text; the
  Document text list now displays actual section paths instead.
- Layout follow-up (not yet a release gate): a read-only, page-1-only geometry probe of a
  user-authorized local PDF showed two body columns, a lower left-column horizontal separator,
  smaller text below it, and right-column body text continuing alongside that lower text.
  The page-bounded word extractor ordered left body, lower note, then right body. The baseline
  `lopdf` converter has no glyph geometry, font metrics, or vector-rule information. Docling PDF
  internally detects a `footnote` region and computes reading order, but version 1.36 assembles
  `footnote` as a plain `Node::Paragraph`; its public `Pipeline::convert` returns no role or
  font/rule geometry. Our mapper also drops `Node::Located` bounding boxes. Neither path can
  reliably distinguish a ruled footnote from ordinary small print at the adapter seam alone.
  A future layout-aware regression should create a synthetic one-page, two-column PDF with
  left/right body boxes, a lower left horizontal rule and smaller text beneath it, plus
  continued right-column body alongside; assert left body then right body then a distinct
  footnote unit/chunk with page provenance. Preserve the layout role from a verified upstream
  renderer or an equivalent supported API; do not infer footnotes from font size alone.
