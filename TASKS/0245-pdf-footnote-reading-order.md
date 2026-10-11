# 0245 PDF footnote reading order and separation

Status: open
Priority: medium
Subsystem: backend, semantic conversion
Depends on: 0180, 0192

## Context

A user reported a two-column article whose smaller-print footnotes sit below a horizontal rule
at the bottom of the left column. The current extracted reading sequence runs from left-column
body into the note, then resumes right-column body. This is a tracked follow-up, **not** an
experimental Gemma release gate; the independently approved large-PDF text and PDF-vision gates
in 0244 remain unchanged.

The user authorized a read-only probe of the *first page only* of their local PDF. No private
text, path, image, or screenshot was retained. The page has two body-text columns, a lower left
horizontal rule, smaller text beneath it, and right-column body text continuing alongside the
note. A page-bounded word extractor ordered lower content as left body → left note → right body.
The underlying PDF reader opens the file's cross-reference/catalog but only extracts/renders
page 1 content.

## Acceptance Criteria

- Build a public-safe, bounded synthetic one-page PDF fixture with left and right body columns,
  a horizontal rule above smaller lower-left footnote text, and right-column body continuing
  below that rule. No private PDF bytes or passages enter the fixture, test output, or repository.
- Extract main text in column order (left body then right body), followed by a distinct
  footnote unit/chunk with page provenance. Keep the note searchable; do not silently discard it,
  join it into a body paragraph, or misattribute its page.
- Use a verified layout role and/or combined font, box and rule geometry. Do not classify
  footnotes from font size alone; protect small-print body and caption content with negative
  fixtures, and cover page transitions and cancellation/resource limits.
- Verify baseline and optional advanced paths explicitly. If a path lacks the required signals,
  fail/disclose its limitation instead of claiming layout-correct extraction.

## Implementation Notes

- `fm-semantic-conversion/src/formats/pdf.rs` uses `lopdf` text extraction and blank lines,
  without glyph metrics, vector-rule geometry, or reliable column role.
- Docling PDF 1.36 detects the `footnote` layout class internally and computes page reading
  order, but its public `Pipeline::convert` assembles footnotes as ordinary `Node::Paragraph`.
  `Node::Located` retains only bounding boxes; font metrics, separator-rule geometry, and
  original layout class do not reach `fm-semantic-docling::Mapper`, which currently also drops
  those boxes. The public page-processing API also returns assembled nodes, not raw region
  labels. A supported upstream role-preserving API or verified renderer/fork may be needed.
- Use synthetic layout such as page 600×800, body columns x=80–280 and x=320–520, a left
  separator at y=610, smaller footnote text at y=625–720, and right-column body extending to
  y=725. These coordinates are illustrative, not copied from the user document.
- Reindex affected existing PDF vectors only after authorization and a versioned pipeline
  change; never trigger user-data reindex as part of diagnosis.

## Agent Notes

- 2026-10-11: Confirmed the page-1 layout and page-bounded extracted category sequence without
  exposing private text. An accidental transient raster written to the worktree by a Poppler
  option was immediately removed; subsequent raster analysis used stdout/in-memory bytes only.
  Adapter-only recovery of a reliable footnote role is not available in Docling 1.36's public
  assembled-node API. Next step is a synthetic failing fixture and a separately scoped
  role-preserving renderer/API change, not a font-only heuristic or release-gate change.
