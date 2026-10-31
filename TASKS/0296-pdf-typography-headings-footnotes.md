# 0296 Typography for the geometric PDF path: headings, citation markers, footnotes

Status: implemented; retrieval A/B negative on 33 questions (open decision)
Priority: medium
Subsystem: backend
Depends on: 0285, 0295

## Context
The text-layer path emitted every line as a flat paragraph: no headings (chunks could not start at
section starts), citation superscripts glued into prose (`lifespan.5`), footnote text trailing the
last paragraph of a page. `docling-pdf` reads fonts while parsing glyphs but keeps them private
(`glyph_styles` is `pub(crate)`), and the user does not want the ML pack.

## Implementation
- `fm-semantic-docling/src/pdf_styles.rs`: own `lopdf` content-stream reader. One `Run` per
  text-showing operator: position, size on the page (text and page matrices, `Ts` rise, form
  XObjects), bold/italic (font name incl. Adv `.B`/`.I` suffixes, descriptor flags/weight), font
  object id, mean letter width, decoded text (ToUnicode, else ASCII for simple fonts). 95-99% of
  docling word cells are covered by a run on the corpus.
- `fm-semantic-docling/src/structure.rs`: `classify(nodes, styles) -> Vec<(Node, Role)>` after
  reflow. Heading: larger than body (x1.12), or bold where the body is not, or - for opaque font
  names - a wider face than the body face (<= 12 words); short, no caption/label, not a sentence,
  starts with a capital/digit, >= 85% of the text in the style; a heading joined to the paragraph
  below it is split off; a heading wrapped over two boxes is joined; 3+ consecutive headings are
  front matter and demoted. Level = rank of the distinct styles (>= 2 uses), max 4.
  Markers: raised small runs (digits, `,`, `-`, `*`, a single lowercase letter) are removed from
  paragraph text, anchored on the last 6 characters of the text they follow, each once per page.
  Footnotes: smaller than body (x0.93), lower half of the page, >= 20 chars / 3 words, starts with
  a symbol or number (a number must match a raised marker on the page, or a leading raised run),
  body text above; wrapped lines after a footnote stay footnotes. They become units under the
  current section + `Footnotes`, so the chunker keeps them apart from the prose.
- Pipeline identity `docling-pdf/2052009+baseline/13`.

## Evaluation
`scripts/eval/pdf-gold/structure_gold.json` (hand-labelled from page images; 14 pages over 12
documents incl. nine added PDFs: arXiv papers, Nature, PLoS, Dobbs, Wikipedia NL/EN) and
`structure_score.py`. Headings P 1.00 / R 0.91, footnotes P 0.71 / R 0.71 (misses and extras are
wrapped footnote lines reflow splits), markers leaked 0/9. Small gold: treat as a smoke test.
Known gaps: italic-only headings; headings inside a table region; fonts without ToUnicode keep
their markers (cannot be anchored); footnote text that reflow cuts into pieces; Roman or
bare-number section marks.

## Retrieval A/B (e5-small, 33 questions, Leiden corpus, same chunker)
| max tokens | chunks base -> new | hit@1 base -> new | MRR base -> new |
|---|---|---|---|
| 256 | 4515 -> 5208 | 0.24 -> 0.24 | 0.380 -> 0.363 |
| 384 | 3017 -> 3471 | 0.33 -> 0.24 | 0.432 -> 0.317 |
| 512 | 2294 -> 2639 | 0.42 -> 0.30 | 0.492 -> 0.389 |
| 768 | 1566 -> 1858 | 0.30 -> 0.27 | 0.396 -> 0.350 |
| 1000 | 1266 -> 1572 | 0.24 -> 0.30 | 0.345 -> 0.414 |
Headings add ~15-25% more chunks (short sections become short chunks: <256 tokens 28% -> 34% at
400/512). e5 (512 limit) gets worse at its own size, a larger-context model gets better at 1000.
33 questions, evidence phrases written against the old text: noise is several points, but the
512 drop is not obviously noise. Next: merge sections under ~120 tokens into their neighbour in
the chunker (keeping the heading in the section path), then re-run.
