# PDF conversion gold standard

Compares Procyon's PDF conversion (text layer + geometric table/reflow rules, and the optional ML
pipeline) against an independent reading of the same pages by
[MiniCPM-OCR 1.0](https://huggingface.co/AtomicChat/MiniCPM-OCR-1.0-GGUF) (Apache-2.0, 1.3B vision
model). Evaluation tooling only; nothing here ships in the app. Context: task 0285.

**The model is a second opinion, not ground truth.** It can misread or invent characters, and its
card reports ~21% character error against human annotation on OmniDocBench (largely format
differences). It was trained on Chinese and English; Dutch and other Latin-script pages are
untested. Hand-check a sample and report the scores on those pages separately (`--verified`).

## Setup (one time)

```bash
brew upgrade llama.cpp        # needs b11826+ (MiniCPM-OCR support); `llama-server --version`
hf download AtomicChat/MiniCPM-OCR-1.0-GGUF MiniCPM-OCR-1.0-AD-Q5_K_M.gguf \
  mmproj-MiniCPM-OCR-1.0-Q8_0.gguf --local-dir ~/models/minicpm-ocr     # 0.59 + 0.62 GB
llama-server -m ~/models/minicpm-ocr/MiniCPM-OCR-1.0-AD-Q5_K_M.gguf \
  --mmproj ~/models/minicpm-ocr/mmproj-MiniCPM-OCR-1.0-Q8_0.gguf --jinja -c 16384 -ngl 99
```

Other tools: `pdftoppm`/`pdfinfo` (poppler) and `uv`. Dependencies are declared inline in each
script (`rapidfuzz`, `pillow`).

## Run

```bash
CORPUS=~/Downloads/"Custom Chat Leiden"
# 1. pages as images (all pages, or --sample 6 evenly spaced pages per PDF)
uv run render_pages.py --pdf-dir "$CORPUS" --out /tmp/pages --sample 6 --tables tables.json

# 2. MiniCPM transcription: pages -> /tmp/gold/<pdf>/p-0007.txt, tables -> /tmp/gold/tables/<id>.html
uv run transcribe.py --pages /tmp/pages --out /tmp/gold --tables tables.json

# 3. Procyon output as JSON lines (text-layer path; no model files needed)
PROCYON_DUMP_DIR="$CORPUS" PROCYON_DUMP_OUT=/tmp/dump \
  cargo test --release -p fm-semantic-docling --test corpus_dump -- --ignored dump_text_layer
#    optional ML path: same command with `--features ml dump_ml` and the ONNX Runtime / PDFium /
#    model environment from docs/docling-pdf-evaluation.md (ORT_SKIP_DOWNLOAD=1, ORT_LIB_PATH, ...).

# 4. score
uv run score.py --gold /tmp/gold --dump /tmp/dump --tables tables.json --verified verified.txt
```

`tables.json` lists the tables to compare (see `tables.example.json`): the PDF stem, the page, and a
box as fractions of the page (`[left, top, right, bottom]`). `transcribe.py` saves the crop as
`<id>.png` next to the model's HTML so you can check the box and the result by eye.

## Hand-verifying

Correct a transcription by saving the fixed text as `p-0007.verified.txt` (or `<id>.verified.html`)
next to the model output; scoring prefers it. List the pages you checked in `verified.txt`, one
`<pdf>/p-0007` per line, and the report adds a "hand-verified pages" row per method. A good sample is
10-15 tables plus ~20 pages covering two-column layouts, reference lists and statistics tables.

## Reading the report

| Metric | Meaning |
|---|---|
| words F1 | bag-of-words overlap with the gold page, ignoring order (did the text survive) |
| reading order | word-sequence similarity, 1 - normalized Levenshtein (is it in the right order) |
| para precision / recall | unit boundaries vs gold paragraph boundaries, by each paragraph's first 4 words |
| units/gold | units per gold paragraph; 1.0 is ideal, above 1 means fragmentation |
| cell F1 / rows found | table cells as a multiset of normalized cells; share of gold rows matched (fuzzy >= 0.9) |

Paragraph metrics depend on how the model separates paragraphs (blank lines, else one per line);
treat them as relative between methods, not absolute. Tests were run with a stub server and with
`pdftotext` output as pseudo-gold only: the pipeline is exercised, but no real MiniCPM run has been
scored yet.

## Page confidence

`dump_confidence` (same test binary as the dumps) writes `<dump>/confidence/<pdf>.jsonl`, the
geometric reconstruction's own trust score per page. Check that it predicts quality:

```bash
uv run score.py --gold /tmp/gold --dump /tmp/dump --page-scores /tmp/pages.jsonl
uv run confidence.py --dump /tmp/dump --page-scores /tmp/pages.jsonl
```

Pages whose gold and output disagree on the words themselves (`--min-words-f1`) are skipped.
