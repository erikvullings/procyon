# Docling.rs PDF Adapter audit and promotion gate

Task 0192 evaluates an optional PDF Adapter; it does not replace Procyon's always-available
converter until the candidate passes this gate.

## Audited source

| Item | Pin |
| --- | --- |
| Project | `docling-project/docling.rs` |
| Release | `1.36.0` |
| Source commit | `660b312780d919a5e29eb9386f2ff8d1a522f8a0` |
| License | MIT, copyright 2026 Artem Kustikov |
| `docling-core` crate SHA-256 | `190122d1b99fc40a1b64595d3275a9fc26075143dbc9f57fd89c236bf2e292d9` |
| `docling-pdf` crate SHA-256 | `4c0b863e5a9356be7aa085bf2d75e5b36ffea9e0a19ee5824a29ba05ef4ce133` |
| `docling-onnx` crate SHA-256 | `33e784a999dcc8e4c697f10831105944e4f9f6321ccf1a1b0c01f17b00aa7193` |

The source commit is not signed. Crates.io checksums therefore pin the evaluated source archives,
but are not an upstream author signature. Procyon must sign the assembled advanced pack with its
own release key.

The Adapter imports only `docling-core` and `docling-pdf`; enabling its `ml` feature brings
`docling-onnx` transitively. It does not import Docling RAG, server, CLI, Python, Node, FFI, WASM,
HEIF, CUDA, TensorRT, DirectML, or CoreML Modules. The audited crates require Rust 1.88 or older;
Procyon's pinned Rust 1.97 toolchain is compatible.

## Native and model supply chain

The ML path adds PDFium, ONNX Runtime `2.0.0-rc.13`, image processing, tokenizers, layout
inference, PaddleOCR recognition, and TableFormer. Upstream enables ONNX Runtime's
`download-binaries` feature and its install scripts download PDFium and models without a
Docling-owned signed manifest. Procyon does not use either download path for release packs.

A release build must:

1. Acquire PDFium, ONNX Runtime, layout, OCR detector/recognizer/dictionary, and TableFormer model
   files in a controlled packaging job.
2. Record every source URL, immutable version, license, and SHA-256 in the signed pack inventory.
3. Build with `ORT_LIB_PATH` pointing at the verified ONNX Runtime, `ORT_PREFER_DYNAMIC_LINK=1`,
   and `ORT_SKIP_DOWNLOAD=1`. `ORT_SKIP_DOWNLOAD=1 cargo check -p fm-semantic-docling --features
   ml` is the network-free compile check.
4. Install the complete inventory atomically. `DoclingPdfBackend::try_ml` rejects an incomplete
   installed inventory before loading a model.
5. Run with no network authority. Model and runtime resolution is confined to installed files.

The candidate targets are `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, and
`aarch64-unknown-linux-gnu`, subject to an artifact smoke test on each target. Upstream does not
currently publish or continuously exercise the complete ML pipeline on all of these targets;
declaration alone is not release evidence.

## Conversion behavior

The no-ML Implementation uses Docling's pure-Rust text parser and reading-order assembly. It emits
page-aware paragraphs but does not infer headings, lists, tables, or OCR. The ML Implementation
enables heading hierarchy, complex layout, local OCR, and TableFormer. Procyon's Adapter:

- maps Docling nodes into Procyon-owned structural units without leaking Docling types across the
  `AdvancedConverterBackend` Seam;
- keeps exact page and reading-order block citations, but reports provenance as approximate because
  Procyon's current model does not retain Docling's region coordinates;
- retains heading hierarchy in `section_path`, lists, code, formulas, field regions, and stable
  tab-separated tables;
- drops Docling furniture layers and exact page-number-only paragraphs while retaining legitimate
  repeated body content;
- exposes OCR language and aggregate confidence as `ConversionWarning::OcrAssessment`;
- records page-level failures as typed `UnreadablePart` omissions when other pages succeed;
- applies source, page, nesting, unit, per-unit, total-output, cancellation, and wall-time limits.

ML conversion uses one-page windows. This bounds resident page raster/tensor data and places
cancellation and timeout checkpoints before and after every page. Docling 1.36.0 has no
intra-page cancellation hook: a single render/inference unit cannot be interrupted cleanly.
Consequently, release promotion also requires an outer semantic-worker watchdog that can terminate
and restart the isolated worker after the hard deadline. This limitation must not be described as
fully cooperative cancellation.

The advanced-first selection policy returns successful advanced PDF output, but keeps the baseline
Implementation as fallback when the pack is absent, incompatible, or has a recoverable runtime
failure. Cancellation, resource-limit, and encryption outcomes are never hidden by fallback.
Pack activation returns the signed affected-format set (`pdf`) and migration impact so the host can
request explicit PDF reindexing while retaining the previous pack for rollback.

## Reproducible evaluation corpus

Committed tests generate deterministic text-layer, multi-page, page-limited, malformed, and
oversized inputs without downloading fixtures. The release corpus must additionally contain
redistributable fixtures and expected reading order for:

| Fixture | Required assertions |
| --- | --- |
| Two-column digital PDF | block order, no cross-column sentence joins |
| Repeated header/footer | furniture removed, repeated body text retained |
| Heading and nested list | heading levels and `section_path`, list order |
| Ruled and borderless tables | row/column order, headers, spans, citation page |
| Image-heavy digital PDF | captions retained, uncaptioned images disclosed |
| English scan | OCR text, language, confidence, word/character error rate |
| Multilingual scan | OCR text, language, confidence, word/character error rate |
| Malformed and encrypted PDF | typed outcome, no crash or partial publication |
| Oversized/adversarial PDF | byte/page/pixel/tensor/time/memory limits |

Run the same corpus through baseline, deterministic Docling, and ML Docling. Record reading-order
accuracy, heading F1, table cell F1, boilerplate/low-information rate, OCR word and character error
rates, citation precision, p50/p95 latency, peak RSS, installed bytes, retrieval nDCG/recall, and
grounded-Ask citation correctness. Use Python Docling only as an offline quality oracle.

## Current promotion decision

Upstream reports 6 of 14 PDF conformance fixtures exact and 7 of 14 equal after whitespace
normalization. Those numbers are self-reported, not Procyon corpus results. The project is young,
high-churn, and effectively single-maintainer; the audited commit is unsigned. The deterministic
Adapter is accepted as an optional spike, but the ML pack is **not promoted** until:

- Procyon-owned, signed per-platform artifact inventories exist;
- the outer hard-deadline watchdog is exercised;
- macOS, Windows, and Linux artifact smoke tests pass; and
- the corpus demonstrates a material retrieval and grounded-answer gain at acceptable resource
  cost.

No quality, latency, memory, or package-size value may be entered in a signed pack manifest before
it is measured on that reproducible corpus.
