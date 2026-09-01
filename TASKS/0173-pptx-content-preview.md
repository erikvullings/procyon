# 0173 PPTX content preview

Status: done
Priority: low
Subsystem: backend, frontend
Depends on: 0088

## Context

The F3 viewer needs a visually faithful PowerPoint preview without requiring LibreOffice or another
external office suite. The first semantic Markdown implementation proved too limited for real
presentations. Convert PPTX packages to PDF in native Rust and display the result through the
existing PDF.js viewer.

Vendor the unpublished, version-aligned `ooxmlsdk` renderer packages behind a small
`fm-pptx-renderer` adapter crate. This keeps upstream source isolated and replaceable while exposing
no third-party types to the application or transport layers.

## Acceptance Criteria

- F3 recognizes `.pptx`, converts it to PDF, and opens the result in the existing read-only PDF
  viewer in presentation order.
- The preview preserves the renderer's slide geometry, text, fonts, shapes, and images rather than
  flattening the deck to semantic Markdown.
- Previous/next controls, keyboard navigation, page count, search, and current-page state are the
  existing PDF viewer behavior.
- Parsing is provider-neutral and implemented behind `fm-application`; HTTP and Tauri adapters are
  thin and behaviorally equivalent.
- Package relationships cannot trigger scripts, arbitrary file reads, or uncontrolled network
  requests.
- Explicit source-byte, expanded-ZIP, entry-count, XML-depth, slide-count, text, media, decoded-image
  pixel, and rendered-PDF budgets prevent unbounded work. Over-budget or unsupported presentations
  retain an external-open fallback.
- Rendered-PDF output and retained sessions are explicitly bounded. Unsupported or over-budget
  presentations retain the external-open fallback.
- Search, cancellation, source-revision invalidation, cleanup, and external-open actions behave
  consistently with other paged F3 renderers.
- Tests cover real PPTX-to-PDF conversion, package limits, cancellation, source changes, bounded PDF
  range reads, cleanup, and HTTP/Tauri/mock parity.

## Implementation Notes

- Keep the vendored renderer behind `fm-pptx-renderer`; application and transport code depend only on
  its byte-oriented adapter.
- Retain converted PDF bytes in the application session and expose bounded range reads to both
  runtime adapters.
- Return an 8 MiB-bounded first-slide PDF from session creation, render the complete deck in the
  background, and replace the initial document when the complete bounded range stream is ready.
- Reuse the existing PDF.js state, rendering, search, and paged controls instead of maintaining a
  PPTX-specific frontend renderer.
- Do not introduce LibreOffice or another external office suite as a required runtime dependency.
- Renderer source provenance and aligned revisions are recorded in `third_party/README.md`.

## Agent Notes

- 2026-08-29: Created from the Office-preview design discussion. This deliberately scopes a useful
  semantic slide reader rather than a PowerPoint-compatible renderer; visual fidelity remains an
  external-application concern.
- 2026-09-01: Implemented a provider-neutral `PptxPreviewService` using `pptx-to-md`, with
  application-owned presentation ordering, bounded package/media parsing, retained opaque
  resources, source-revision checks, cancellation, and equivalent HTTP/Tauri/mock adapters.
- 2026-09-01: Added the paged F3 content viewer with sanitized per-slide rendering, titles, text,
  lists, tables, links, notes, images, search, copy, keyboard navigation, explicit fidelity limits,
  cleanup, and external fallback. Covered parser safety/lifecycle, host routes/adapters, and viewer
  behavior with focused Rust and frontend tests.
- 2026-09-01: Replaced the semantic Markdown implementation with native PPTX-to-PDF rendering using
  vendored `ooxmlsdk` source behind `fm-pptx-renderer`. The application now retains a bounded PDF
  session, HTTP/Tauri/mock adapters expose bounded range reads, and the frontend reuses its PDF.js
  viewer for visual slide fidelity, navigation, and search.
- 2026-09-01: Added deterministic missing-family fallback for Aptos/Aptos Display presentations,
  first-slide-first background conversion, explicit renderer/budget failure messages, and common F3
  Tab/PageUp/PageDown behavior for PDF/PPTX and DOCX previews.
