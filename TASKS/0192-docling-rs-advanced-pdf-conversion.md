# 0192 Deterministic Docling.rs PDF conversion

Status: done
Priority: high
Subsystem: backend, search
Depends on: 0189

## Context

The original `lopdf` converter does not reconstruct positioned multi-column reading order, which
reduces semantic-search and grounded-Ask quality. Use the official MIT-licensed
[`docling-project/docling.rs`](https://github.com/docling-project/docling.rs) pure-Rust text-layer
pipeline as Procyon's default PDF converter. Retain the original converter as a recoverable
fallback. OCR, PDFium, ONNX Runtime, and model packs are optional follow-up work and must not block
this deterministic integration.

## Acceptance Criteria

- Pin and audit a specific `docling.rs` release or commit and use only its deterministic
  `docling-core` and `docling-pdf` surfaces in normal builds. Conversion performs no runtime
  downloads and adds no native runtime or model dependency.
- Implement the PDF Adapter behind `AdvancedConverterBackend`; ingestion, application conversion,
  reconciliation, search, and Ask continue to depend only on `DocumentConverter`.
- Adapt output into Procyon's versioned structural, provenance, warning, and omission types without
  leaking Docling-owned types or claiming unavailable region precision.
- Make deterministic Docling the preferred production PDF converter in both application and worker
  composition roots. Recoverable failures fall back to the existing baseline converter;
  cancellation, encryption, and resource-limit outcomes remain typed.
- Preserve page-aware, position-derived reading order and suppress page furniture and
  page-number-only artifacts. Generated two-column coverage proves the production paths do not
  regress to baseline stream order.
- Enforce source-byte, page, nesting, unit, output, cancellation, and wall-time limits. Malformed
  and oversized inputs cannot publish partial converted output.
- PDFs without searchable text are intentionally excluded, not failed or cancelled. The host-facing
  indexing report includes actionable OCRmyPDF guidance and does not count the exclusion as a
  retryable failure.
- Give the Docling-first pipeline a stable converter identity. Upgrading a baseline index clears
  derived model data exactly once, preserves enrolment policy, and preserves rebuilt data on later
  restarts.
- Tests cover conversion, positioned columns, structural mapping, citations, furniture filtering,
  fallback, deterministic output, limits, cancellation, malformed input, no-text exclusions, IPC
  state propagation, and one-time index migration.

## Implementation Notes

- The isolated Adapter lives in `crates/fm-semantic-docling`; production construction remains a
  small `DocumentConverter` factory with the baseline converter as fallback.
- Upstream references:
  - <https://github.com/docling-project/docling.rs>
  - <https://github.com/docling-project/docling.rs/blob/master/crates/docling-pdf/Cargo.toml>
  - <https://github.com/docling-project/docling.rs/blob/master/docs/PDF_CONFORMANCE.md>
  - <https://docling-project.github.io/docling/concepts/architecture/>
- `docs/docling-pdf-evaluation.md` retains the completed source audit and records the separate,
  optional ML-pack promotion gate. Those ML requirements are not acceptance criteria for this task.

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
- 2026-09-06: Scope corrected after user review: Procyon needs the deterministic Rust Docling
  text-layer converter now; it does not need to promote or independently validate the optional ML
  stack for this MVP. Deterministic Docling is the preferred converter with baseline fallback.
  Textless PDFs are excluded with OCRmyPDF guidance, and optional automatic OCR is tracked by 0193.
  The converter pipeline identity participates in one-time derived-index migration so existing
  baseline chunks cannot be silently mixed with Docling chunks.
- 2026-09-06: Completed production integration in `fm-semantic-docling`, `fm-semantic-worker`, and
  `fm-application`. The versioned IPC now distinguishes intentional exclusions from cancellation
  and carries OCR remediation into the host report. Review follow-up prevents baseline fallback
  from restoring filtered page-only artifacts and deletes stale searchable evidence when a file
  changes to a no-text PDF. Converter identity migration resets baseline-derived data once while
  preserving rebuilt indexes. Connector integration fixtures now own short temp runtimes, worker
  binaries, and native libraries; stale peer teardown is classified separately from insecure
  ownership. Validation: 644 application tests, 251 conversion/Docling/worker tests, 20 repeated
  parallel connector lifecycle runs, and `pnpm run lint`.
