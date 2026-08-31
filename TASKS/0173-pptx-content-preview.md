# 0173 PPTX content preview

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0088

## Context

The F3 viewer has no PowerPoint preview. A faithful slide renderer would require substantial
PresentationML, DrawingML, theme, font, chart, and layout support, but a native Rust parser can
still provide a useful content-oriented preview of slide text and basic media.

Prototype `pptx-to-md` as the first native Rust option. Treat its Markdown as an intermediate
representation only: retain explicit slide boundaries and sanitize the rendered result through the
existing Markdown preview path. If the prototype cannot preserve stable slide ordering, notes, or
image relationships, evaluate `ppt-rs` behind the same application-owned interface rather than
leaking a crate-specific model into transport.

## Acceptance Criteria

- F3 recognizes `.pptx` and opens a read-only in-app preview organized by slide in presentation
  order.
- The preview renders slide titles, text, lists, tables, links, speaker notes, and embedded images
  when the selected parser exposes them, with clear placeholders for omitted content.
- Previous/next controls, keyboard navigation, slide count, and current-slide state reuse the
  existing paged PDF/comic/EPUB viewer conventions.
- Parsing is provider-neutral and implemented behind `fm-application`; HTTP and Tauri adapters are
  thin and behaviorally equivalent.
- Markdown/HTML output is sanitized before rendering. Package relationships cannot trigger scripts,
  arbitrary file reads, or uncontrolled network requests.
- Explicit source-byte, expanded-ZIP, entry-count, XML-depth, slide-count, text, and media budgets
  prevent unbounded work. Over-budget or unsupported presentations retain an external-open fallback.
- The UI labels this as a content preview and does not imply fidelity for themes, precise geometry,
  fonts, transitions, animations, SmartArt, charts, embedded objects, audio, or video.
- Search, selection/copy, cancellation, source-revision invalidation, cleanup, and external-open
  actions behave consistently with other paged F3 renderers.
- Tests cover ordering, titles/lists/tables, notes, images, unsafe links, malformed relationships,
  ZIP limits, unsupported drawing content, cancellation, source changes, and host parity.

## Implementation Notes

- Keep slide structure in the DTO/state model instead of flattening the whole deck into one Markdown
  string; this preserves navigation and bounds frontend memory.
- Reuse `safeMarkdownHtml`, the sanitized preview styling, and paged-content controls in
  `frontend/src/features/preview/file-viewer.ts`.
- Do not introduce LibreOffice or another external office suite as a required runtime dependency.
  Optional future conversion-to-PDF can be considered separately if users require visual fidelity.
- `pptx-to-md` and `ppt-rs` are newer and less established than Calamine. The first implementation
  step is a fixture-based parser evaluation covering real-world decks before transport/UI work.

## Agent Notes

- 2026-08-29: Created from the Office-preview design discussion. This deliberately scopes a useful
  semantic slide reader rather than a PowerPoint-compatible renderer; visual fidelity remains an
  external-application concern.
