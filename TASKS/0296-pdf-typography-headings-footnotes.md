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
- Pipeline identity `docling-pdf/2052009+baseline/14` (13 = first typography commit; 14 = false-heading fixes).

## Evaluation
`scripts/eval/pdf-gold/structure_gold.json` (hand-labelled from page images; 14 pages over 12
documents incl. nine added PDFs: arXiv papers, Nature, PLoS, Dobbs, Wikipedia NL/EN) and
`structure_score.py`. Headings P 1.00 / R 0.91, footnotes P 0.71 / R 0.71 (misses and extras are
wrapped footnote lines reflow splits), markers leaked 0/9. Small gold: treat as a smoke test.
Known gaps: italic-only headings; headings inside a table region; fonts without ToUnicode keep
their markers (cannot be anchored); footnote text that reflow cuts into pieces; Roman or
bare-number section marks.

## Retrieval A/B (e5-small, 33 questions, same 20 Leiden PDFs, same chunker)
First A/B showed headings hurting (512: hit@1 0.42 -> 0.30). **That was a harness bug**:
`retrieve.py` built the attention mask from the padded length, so pad tokens entered the mean
pool and every embedding depended on its batch (rewording one query moved the baseline 512 row
from 0.42 to 0.27). Fixed (tokenizer's own attention mask); table below is the corrected one.
Per-question diff also found real bugs, fixed here: body lines promoted to headings (next
paragraph continues in lowercase, or the line ends on a function word / comma), and a journal
banner + title pair kept as section path ("ORIGINAL ARTICLES > Title"; now an opening run of
2 headings before any body text is front matter). Chunker 6 caps the section prefix in the
embedding input to the innermost sections within 100 chars. Q27 (compound question) reworded.

| max tokens | chunks base / headings / v2 | hit@1 base / headings / v2 | MRR base / headings / v2 |
|---|---|---|---|
| 256 | 4515 / 5208 / 4982 | 0.42 / 0.42 / 0.45 | 0.540 / 0.544 / 0.582 |
| 384 | 3017 / 3471 / 3342 | 0.48 / 0.39 / 0.48 | 0.600 / 0.543 / 0.606 |
| 512 | 2294 / 2639 / 2549 | 0.45 / 0.42 / 0.48 | 0.587 / 0.549 / 0.594 |
| 768 | 1566 / 1858 / 1792 | 0.30 / 0.36 / 0.45 | 0.485 / 0.518 / 0.565 |
| 1000 | 1266 / 1572 / 1500 | 0.45 / 0.36 / 0.36 | 0.573 / 0.536 / 0.548 |
"headings" = first typography commit; v2 = false-heading fixes + prefix cap. v2 is level with or
above the baseline at 256-768; 1000 is lower on hit@1 (33 questions: 1 question = 3 points, so
treat it as noise). Stripping the prefix entirely (`PROCYON_CHUNK_NO_PREFIX=1`, corrected harness): hit@1 / MRR
256 0.48 / 0.598, 384 0.48 / 0.612, 512 0.39 / 0.552, 768 0.39 / 0.542, 1000 0.39 / 0.585 vs v2
above: mixed, so the prefix is neutral at this sample size. Merging short sections is not needed.
