# 0192 Docling.rs advanced PDF conversion

Status: blocked
Priority: high
Subsystem: backend, search, packaging
Depends on: 0189

## Context

The baseline PDF converter in `fm-semantic-conversion` uses `lopdf` text-layer extraction, orders
pages but not positioned text, splits only on blank lines, and emits every block as a paragraph.
It cannot reconstruct multi-column reading order, detect headings or tables, remove repeated page
furniture, or OCR scanned pages. The resulting low-information and out-of-order chunks reduce
semantic-search and grounded-Ask quality even when retrieval thresholds correctly reject the
weakest matches.

Do not port Python Docling from scratch. The official Docling organization now maintains the
MIT-licensed Rust workspace [`docling-project/docling.rs`](https://github.com/docling-project/docling.rs).
Its `docling-pdf` crate offers a text-layer path and an optional PDFium/ONNX pipeline for layout,
OCR, and table recognition. Evaluate and integrate only the PDF Modules needed by Procyon through
the existing `AdvancedConverterBackend` Seam. Keep the small pure-Rust converter as the
always-available fallback rather than making ordinary semantic indexing depend on native model
packs.

## Acceptance Criteria

- Pin and audit a specific `docling.rs` release or commit. Record its license, transitive native
  dependencies, model provenance, supported targets, known conformance gaps, and maintenance risk;
  do not rely on runtime downloads or unverified upstream binaries.
- Establish a reproducible comparison corpus containing text-layer, multi-column, repeated
  header/footer, heading/list, table, image-heavy, scanned/OCR, malformed, encrypted, and oversized
  PDFs. Compare the existing converter, the deterministic `docling.rs` path, and its ML path;
  upstream Python Docling may be used only as an offline quality oracle.
- Record reading-order accuracy, heading hierarchy, table structure, boilerplate/low-information
  rate, OCR quality, provenance precision, conversion latency, peak memory, package size, and
  retrieval/grounded-Ask metrics. Promotion requires a documented material quality gain over the
  0188 baseline, not merely successful conversion.
- Implement a Docling PDF Adapter behind `AdvancedConverterBackend`; ingestion, reconciliation,
  search, and Ask continue to depend only on `DocumentConverter` and do not branch on Docling.
- Restrict integration to the required `docling-core`, `docling-pdf`, and `docling-onnx` Modules or
  a smaller audited subset. Do not import Docling RAG, server, CLI, Python, Node, FFI, or WASM
  surfaces into the worker.
- Adapt Docling output into Procyon's versioned `ConvertedDocument`, `StructuralUnit`,
  `section_path`, table, warning/omission, and provenance types. No Docling-owned type crosses the
  advanced-converter Seam, and output never claims page, region, heading, or table precision that
  the backend did not establish.
- Preserve page-aware reading order, heading hierarchy, lists, tables, and exact/best-available
  source locations. Suppress repeated headers, footers, page-number-only blocks, and demonstrably
  low-information extraction artifacts without deleting legitimate repeated document content.
- Support scanned PDFs through bounded local OCR. OCR language selection and confidence are visible;
  partial OCR/layout/table failures produce typed omissions rather than fabricated success.
- Enforce byte, page, pixel, tensor, output, memory, concurrency, and wall-time limits with
  cancellation checkpoints around rendering and inference. Malformed or adversarial PDFs cannot
  crash the worker, escape the process, access source paths, or leave a partial generation visible.
- Package PDFium, ONNX Runtime, model/tokenizer files, and any other native assets as pinned,
  checksum-verified, signed managed artifacts for each supported platform. Installation is
  consented, reports download/disk/RAM estimates, works offline after installation, and performs no
  runtime code or model download.
- Activating the advanced PDF pack triggers an explicit converter-version reindex plan for affected
  PDFs. Removal, incompatibility, or runtime failure falls back to the baseline converter without
  losing enrolment policy or corrupting the active index; rollback to the previous signed pack is
  supported.
- Tests cover Adapter absence/presence, structural mapping, citations, OCR and text-layer fixtures,
  tables and columns, boilerplate suppression, deterministic output, resource limits,
  cancellation, malformed output/input, pack compatibility, reindex migration, rollback, fallback,
  tenant isolation, and macOS/Windows/Linux artifact smoke checks.

## Implementation Notes

- Primary Procyon files: `crates/fm-semantic-conversion/src/formats/pdf.rs`,
  `crates/fm-semantic-conversion/src/advanced.rs`,
  `crates/fm-semantic-conversion/src/converter.rs`, and
  `crates/fm-semantic-conversion/src/chunk.rs`.
- Upstream references:
  - <https://github.com/docling-project/docling.rs>
  - <https://github.com/docling-project/docling.rs/blob/master/crates/docling-pdf/Cargo.toml>
  - <https://github.com/docling-project/docling.rs/blob/master/docs/PDF_CONFORMANCE.md>
  - <https://docling-project.github.io/docling/concepts/architecture/>
- Treat upstream conformance and performance reports as self-reported until reproduced on Procyon's
  corpus. Audit cancellation behavior and native resource ownership before choosing a dependency
  over a maintained fork.
- Implement in evaluation-gated stages: dependency and corpus audit, Adapter spike, quality/resource
  report, signed cross-platform pack, then explicit activation and reindex migration.

## Agent Notes

- 2026-09-06: Created after real TRIZ-corpus retrieval showed that score filtering can exclude weak
  evidence but cannot repair missing headings, fragmented tables, page furniture, or incorrect PDF
  reading order. Prior investigation found that a from-scratch Rust port is unnecessary because the
  official `docling.rs` project already provides the relevant Rust and ONNX Modules. Begin with an
  independently measured Adapter spike; retain the current `lopdf` Implementation as fallback until
  the advanced pack passes quality, resource, cancellation, malformed-input, and packaging gates.
- 2026-09-06: Implementation started on `semantic-worker-ipc`, where the completed 0177–0189
  prerequisite chain and `AdvancedConverterBackend` Seam exist. The main-based planning session
  cannot implement this task until that chain lands.
- 2026-09-06: Pinned `docling-core`/`docling-pdf` 1.36.0 and added a separate
  `fm-semantic-docling` Module. The deterministic Adapter now proves public-Interface PDF
  conversion, position-aware column ordering, structural mapping, page citations, furniture/page
  number suppression, sanitization, and output/page/depth/cancellation limits. The optional ML
  build type-checks offline and maps headings, lists, tables, formulas, OCR language/confidence,
  and partial page failures. A build guard rejects ML builds unless ONNX downloads are disabled.
- 2026-09-06: Advanced selection can now prefer an installed converter while retaining typed
  baseline fallback. Signed pack schema 2 adds native/model provenance, affected-format migration
  plans, a material nDCG promotion gate, target-specific Docling manifests, and correct no-op
  rollback reporting. The audit and release gate are documented in
  `docs/docling-pdf-evaluation.md`.
- 2026-09-06: Blocked release promotion after code review confirmed an upstream architectural gap:
  Docling 1.36.0 cannot interrupt a render/inference operation within one page, while Procyon's
  worker launcher currently relinquishes the child-process handle and therefore cannot enforce a
  kill-and-restart deadline. The ML pack must remain unpromoted until a killable subprocess
  watchdog is implemented and exercised, Procyon-owned checksum/signature inventories are produced
  for PDFium/ONNX/models on every target, and the reproducible PDF corpus supplies actual
  quality/resource/retrieval measurements. Task 0192 and its README checkbox intentionally remain
  incomplete rather than substituting invented metrics or self-reported upstream conformance.
