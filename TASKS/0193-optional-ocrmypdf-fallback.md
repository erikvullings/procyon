# 0193 Optional OCRmyPDF fallback

Status: completed
Priority: medium
Subsystem: backend, search
Depends on: 0192

## Context

Deterministic Docling converts PDFs that already contain a searchable text layer. Image-only PDFs
are deliberately excluded with instructions to add one using OCRmyPDF. Procyon may optionally
automate that remediation when the user has installed OCRmyPDF, without turning an external
executable into a mandatory or silent dependency.

## Acceptance Criteria

- Automatic OCR is disabled by default and requires explicit configuration or consent.
- Activation occurs only after deterministic conversion reports `NoTextLayer`; ordinary PDFs never
  take the subprocess path.
- The source is copied into bounded private temporary storage. OCRmyPDF receives no provider
  credentials or original source path, and temporary input/output is removed after every outcome.
- The child process has a hard deadline, is terminated on cancellation, and cannot leave a
  background process or partial index generation.
- Successful OCR output is fed back through deterministic Docling and retains typed provenance and
  omissions. Failure keeps the actionable manual OCR guidance.
- Capability detection and platform guidance cover Homebrew on macOS, distribution packages on
  Linux, and WSL on Windows without downloading executables at runtime.
- Integration tests exercise installed, absent, failed, timed-out, cancelled, oversized-output, and
  successful OCR paths using a controlled fake executable; an opt-in local smoke test may use a real
  OCRmyPDF installation.
- Any future Docling ML/PDFium/ONNX alternative remains a separately consented managed pack subject
  to the release gate in `docs/docling-pdf-evaluation.md`.

## Implementation Notes

- Extend the conversion composition boundary rather than adding OCR branches to ingestion, search,
  or Ask.
- Reuse Procyon's existing managed-process and temporary-storage patterns where they satisfy
  kill-on-drop, ownership, and resource-limit requirements.
- OCRmyPDF must never modify the source PDF in place.

## Agent Notes

- 2026-09-06: Split from 0192 when deterministic Docling was accepted as the production MVP.
  OCRmyPDF is installed at `/opt/homebrew/bin/ocrmypdf` on the development Mac, but that local fact
  must not become a product assumption or a test dependency.
- 2026-09-07: Added a default-off OCRmyPDF composition wrapper at the document-conversion boundary.
  It activates through `pnpm dev:tauri:semantic:ocr` (or
  `PROCYON_SEMANTIC_OCRMYPDF=1`), supports a trusted executable override,
  uses private temporary input/output, enforces cancellation and a hard deadline, bounds output
  before reading, starts a dedicated process group so helper processes are terminated too,
  forwards only a small allow-list of non-secret environment variables, and always reconverts OCR
  output through Docling before the baseline fallback.
  Controlled fake-executable tests cover capability detection, absence, failure, timeout,
  cancellation, oversized output, successful OCR, cleanup, and bypass for ordinary searchable
  PDFs. An ignored opt-in smoke test verified the installed macOS OCRmyPDF against a real source
  without changing it.
- 2026-09-07: Final validation passed workspace lint, all 18 non-ignored
  `fm-semantic-docling` tests, the ignored real OCRmyPDF smoke test, all 128
  `fm-semantic-worker` tests (2 ignored), and all 41 script tests. The OCR-enabled release worker
  was activated without changing the global conversion-pipeline identity. Reconciliation of the
  sole enrolled `~/Downloads/triz` root completed 40 jobs with no failures; all six formerly
  textless PDFs now have searchable records.
