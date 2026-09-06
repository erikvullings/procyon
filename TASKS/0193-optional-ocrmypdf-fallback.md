# 0193 Optional OCRmyPDF fallback

Status: open
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
