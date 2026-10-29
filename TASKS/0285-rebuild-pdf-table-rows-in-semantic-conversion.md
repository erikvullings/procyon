# 0285 Rebuild PDF table rows in semantic conversion

Status: in progress (text-layer fallback done; ML pack open)
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
- 2026-10-29: Added `fm-semantic-docling/src/table_rows.rs` (option 1, applied inside `reflow::flush`):
  rows are clustered by vertical centre, a block needs >=3 multi-cell rows, >=8 cells and >=60%
  datum-like cells (so prose and two-column text never qualify); tracks come from the widest row of
  alphanumeric cells, stray marks (`*`, brackets) join their neighbour, gaps are padded with empty
  cells (document_parsing_protocol rules: whitespace tables, never invent, header/body count
  mismatch, marks in cells). Emits `Node::Table` -> `UnitKind::Table`. Pipeline identity bumped to
  `docling-pdf/2052009+baseline/10`. Fixture-PDF regression test in `tests/docling_pdf_adapter.rs`.
  **Still open:** option 3 (ML layout + TableFormer in the signed docling pack, catalog, download
  size, licence disclosure) - needs model artifacts and pack signing; not started. Not verified on
  the real juveniles PDF (not on this machine). Protocol ideas not yet used: multi-row header tiers,
  total/subtotal rows, cross-page table continuation.
- 2026-10-29 (ML probe, macOS arm64, release build, ONNX Runtime 1.28.0 + PDFium chromium/8125 +
  upstream models-v1 layout_heron_int8/TableFormer, SHA-256 checked against upstream SHA256SUMS) on
  `Risk factors ... juveniles_meta analyse.pdf`: ML 13 s, 128 units / 5 tables / 41 under 30 chars;
  geometric text-layer 619 units / 16 tables / 238 under 30 chars. ML keeps captions and header rows
  and gets Tables 1-2 exactly right (Table 1: all 12 columns). It mangles big multi-row-span
  Table 3 (rows shuffled, e.g. `Parent-report 1 16 1 ...`), where the geometric rows stay intact but
  lack headers/captions. Default 30 s budget is exceeded by a debug build (needs release, or a larger
  timeout for ML). ML pack still unshipped: signing, Win/Linux smoke tests, catalog, size/licence
  disclosure remain. Suggested next: ship ML, keep the geometric pass for pages where ML yields no
  or implausible tables, and measure retrieval on the corpus.
- 2026-10-29 (geometric improvements, corpus `~/Downloads/Custom Chat Leiden`, 22 PDFs): table rebuild
  now attaches captions, keeps lone signs/brackets with their values, joins wrapped row labels,
  and ignores rows holding a sentence. Reflow (`reflow.rs`) now: continues hanging-indent entries
  (reference lists) and indented first-line paragraphs, rejoins fragments painted on one line
  (`X`, `=`, `definition`), drops punctuation-only lines and running headers/footers repeated on >=3
  pages. Corpus units 18,122 -> 12,948, units under 30 chars 8,290 -> 4,022, average unit 96 -> 134
  chars (ML pipeline: 4,683 units, 1,159 short). Table row comparison vs ML (weak yardstick: ML
  itself garbles some tables): numeric-token agreement ~54%, exact rows ~34%; captions 22 vs ML 60.
  Remaining short units: stray table remnants, headings, page-top labels, merged column text
  ("Author. Banducci" - two entries in one text-layer line, unfixable at this layer). Next ideas: a
  gold standard (hand-labelled tables + page-level transcription) before deciding on the ML pack.
- 2026-10-29 (gold standard, `scripts/eval/pdf-gold`, MiniCPM-OCR 1.0 AD-Q5_K_M via llama.cpp 0.8.0,
  207 pages of the corpus incl. all table pages, 7.8 s/page): on pages both methods converted,
  words F1 text-layer 0.90 / ML 0.90 (content survives equally); reading order 0.68 / 0.78;
  paragraph precision 0.36 / 0.61; units per gold paragraph 6.6 / 1.35; model-found tables (13):
  cell F1 0.30 / 0.64. The text layer produced nothing for 29 pages (5 PDFs, scans or partly
  scanned), ML read all but one. Found and fixed: on two-column pages the text layer's cells carry
  words of both columns, so `reflow.rs` now rebuilds such pages' lines from the word boxes (gutter
  from word coverage, rows cut at the gutter, spanning titles kept whole, tables and running
  headers preserved). Reading order 0.66 -> 0.68. Biggest remaining text-layer gap: text-heavy
  tables (e.g. continued multi-column review tables) are read column by column because the table
  gate only accepts datum-like cells. MiniCPM is a second opinion, not ground truth: hand-verify
  a sample before trusting small differences.
- 2026-10-29: Text-heavy tables (`fm-semantic-docling/src/table_text.rs`, run from `reflow::flush`
  after the numeric gate, only with word boxes). Gate: >=3 and <=8 columns separated by vertical
  corridors (>=6 page units, <=1% of words touching), each column >=3% of words; region = below a
  `Table N` caption, else the page body (running headers drop repeated captions, so continuation
  pages have none). Lines crossing a corridor at the top/bottom (wrapped caption, note) are cut;
  word runs (gap <=4 units) join the column they start in (centred headers). Rows break at gaps
  >1.35x line pitch; equal-pitch records fall back to repeated first-column labels (`Item 1`...).
  Identity `.../baseline/11`. Gold (text-layer, 177 pages): reading order 0.683 -> 0.721, units per
  gold paragraph 6.6 -> 6.0, FFP 0.837 -> 0.916 (ML 0.904), Radicalisation 0.498 -> 0.620 (ML
  0.694); table cell F1 on model-found tables unchanged (0.36 vs ML 0.64). Known limits: a page
  mixing prose and a table without a caption is not detected; forest-plot pages with side prose
  (Radicalisation p54) become a table with prose cells; tables where only the header/records
  separate by gap and rows <3 are skipped. Next: ruling-line grids, bands, confidence.
