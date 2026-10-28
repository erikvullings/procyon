# 0285 Rebuild PDF table rows in semantic conversion

Status: open
Priority: medium
Subsystem: backend
Depends on: none

## Context
PDFs converted through the Docling text-layer path (the only path in the developer and production
bundles today) give statistics tables no structure: every table cell becomes its own one-line
paragraph (`0.145 (`, `*`, `0.174, 0.463)`, `.070`). Chunks built from them are number soup. Seen in
`Risk factors for persistent delinquent behavior among juveniles_meta analyse.pdf` (pages 6-9): before
the `/9` converter fixes, 336 of 522 chunks were under 60 characters.

Search still returns these chunks. Corpus themes now exclude them (`is_low_information` in
`fm-semantic-conversion/src/pdf_toc.rs`), but that only hides the symptom for themes.

### Is this a docling.rs problem?
No, by design. `docling_pdf::convert_text_layer_pages` is documented as flat, line-grouped paragraphs
"no headings/lists/tables/pictures" (`docling-pdf` `src/lib.rs`, same text in 2.52.9 and 2.53.3).
Python Docling's table support corresponds to docling.rs's **ML pipeline** (`ml` feature: layout
detection + TableFormer, models required), which `fm-semantic-docling` already has as `convert_ml`
behind `ml`. The ML pipeline also has a geometric table reconstruction fallback when TableFormer
graphs are absent (`assemble.rs`, issue #157 upstream; public under the `ocr-prep` feature).

Version: the workspace pins `docling-core`/`docling-pdf` to `=2.52.9`. 2.53.3 was published on
2026-10-28 and has not been tried. Upgrading changes the converter identity
(`docling-pdf/2052009` -> 2053003), the docling pack version (`DOCLING_RS_VERSION` in
`fm-semantic-components/src/docling_pack.rs`, currently unverified) and needs a fresh production
qualification.

## Acceptance Criteria
- A PDF table page converts to table-shaped units (`UnitKind::Table`, tab-separated rows like the
  existing ML mapping) instead of one paragraph per cell, verified on the juveniles meta-analysis
  PDF (pages 6-9) with a regression test using a small fixture PDF.
- Search chunks for those pages carry whole rows, not single cells.
- The `0.145 ( * 0.174, 0.463)` and `.070` fragments no longer appear as chunks of their own.
- The converter identity is bumped so indexed documents rebuild.
- The layout and TableFormer models are part of the signed docling pack (catalog, download size and
  licence disclosure updated), and PDFs without a usable ML result still fall back to the text layer.

## Implementation Notes
**Decision (user, 2026-10-28): option 3, the ML pipeline with TableFormer.** The package must ship the
layout and table models and the `ml` feature; options 1 and 2 are kept for reference only.

Options, cheapest first:
1. Rebuild rows ourselves in `fm-semantic-docling/src/reflow.rs` from word cells
   (`page_words`, already used for bracket reattachment): cluster cells by top coordinate into
   rows and by left edge into columns, emit a `Table` unit when a page region is dominated by
   numeric/short cells. Needs a reliability gate like upstream's to avoid turning prose into tables.
2. Reuse upstream's geometric table reconstruction from `docling_pdf::assemble` (needs the
   `ocr-prep` feature; check what it requires and whether it is usable without the ML models).
3. Use the ML pipeline (`ml` feature, TableFormer) for pages with table-like content: best
   quality, but needs layout and table models and pack changes.
Try upgrading to docling-pdf 2.53.3 first and re-run the text-layer probe; read the upstream
changelog for table or text-layer changes before deciding.
- Related: `crates/fm-semantic-docling/src/lib.rs` (`Mapper`, `convert_text_layer`, `convert_ml`),
  `crates/fm-semantic-conversion/src/chunk.rs` (chunker packs adjacent units per page), task 0284.

## Agent Notes
- 2026-10-28: Confirmed on the real PDF with a temporary probe: text-layer output has one paragraph
  per cell. Fixed related regressions on the way: the bare-outline-number rule no longer flags
  decimals like `25.1` (that split packing and gave 522 chunks, now 177), and empty units after
  dot-leader cleanup are dropped (they made the adapter reject the document and fall back to the
  baseline extractor). Not tried: docling-pdf 2.53.3, geometric reconstruction, the ML pipeline.
