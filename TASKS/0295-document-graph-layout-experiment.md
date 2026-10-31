# 0295 Document-graph layout experiment (glyph graph + whitespace edges)

Status: done (experiment; negative/inconclusive for prose, not ported to Rust)
Priority: low
Subsystem: backend (eval)
Depends on: 0285

## Context
Evaluate a multi-relational document graph (glyph NN edges, typographic edges, whitespace edges,
superscript edges; Docstrum/PdfPig/CERMINE/GLAM lineage) against the geometric text-layer path in
`fm-semantic-docling`, using the MiniCPM gold of `scripts/eval/pdf-gold`.

## Result
Prototype: `scripts/eval/pdf-gold/graph_layout.py` (Python/PyMuPDF; needs glyph, font, size and
baseline data that `docling_pdf::textparse::pdf_words` does not expose - word boxes only).
Gold regenerated: 138 pages (6 evenly spaced per PDF, 22 PDFs of `Custom Chat Leiden`), 110 scored.
Tables not covered.

| method | words F1 | reading order | para P | para R | units/gold |
|---|---|---|---|---|---|
| graph-A spatial only | 0.916 | 0.787 | 0.411 | 0.438 | 3.2 |
| graph-B + typographic gates | 0.915 | 0.787 | 0.427 | 0.567 | 4.8 |
| graph-C + whitespace corridors | 0.914 | 0.731 | 0.417 | 0.671 | 6.9 |
| text-layer (current) | 0.909 | 0.796 | 0.437 | 0.670 | 4.3 |

- Typographic edges (size/weight gates) raise paragraph recall 0.44 -> 0.57 at no cost.
- Corridor edges reach baseline paragraph recall but lose reading order, mostly on table-heavy
  pages (columns read separately) - the existing `regions.rs`/`table_text.rs` handle those.
- Nothing beats the current path overall. The corpus has ~no superscripts/footnotes/math
  (sup edges: 0 real), so the graph's main selling point was untestable here.
- Prototype got two bugs on the way (inverted corridor test, rounding at corridor walls): numbers
  are a first cut, not tuned.

## Follow-up results (2026-10-31)
- Font gate ported to `reflow.rs` (previous line's box height vs next, thresholds 2u/1.15, 3u/1.3,
  2u/1.4) and scored on the same gold: words F1 0.909, order 0.796, para P 0.437, R 0.670-0.672,
  units/gold 4.33 - identical to the baseline within noise. The existing indent/gap/fullness rules
  already break at headings and captions, so the Python gain (0.44 -> 0.57 recall) was measured
  against a weaker spatial-only baseline. Change reverted. Bold/family are not exposed (glyph
  styles are `pub(crate)` in docling-pdf), so only size could be tested.
- Superscripts: the first probe undercounted. Only `The Impact of Attachment-Disrupting ...` uses
  numeric citation superscripts at scale (106 sup edges); ~25 more across the other PDFs, mostly
  statistic marks (`I2`, `chi2`). The gold writes them as `\(^{21}\)`; the graph prototype emits
  `^21`. Fixed a prototype bug that merged markers into the following word. Footnote *blocks*
  (small text at page bottom) appear on a few pages (e.g. `development of the a typical mind`)
  but social-science PDFs here mostly cite author-year. Six scanned PDFs have no text layer and
  were not probed.
