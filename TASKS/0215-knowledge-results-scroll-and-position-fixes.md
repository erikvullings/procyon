# 0215 Knowledge results scroll and position fixes

Status: done
Priority: high
Subsystem: frontend, semantic search, preview
Depends on: 0214

## Context

The centred file-preview progress indicator introduced in task 0214 renders as a static circle
because `CircularProgress` depends on styles from the package's aggregate stylesheet, while Procyon
deliberately loads only the package's modular stylesheets. Search Knowledge also places its results
scrollport inside the section's right padding, leaving the scrollbar inset from the pane edge.

Production chunk provenance serializes spans as
`{"kind":"span","value":{"first":...,"last":...}}`. The current label parser recognizes only
the older top-level `first`/`last` shape, so span chunks omit their page link even though their
indexed provenance contains page numbers.

## Acceptance Criteria

- Render a visibly animated, centred file-preview spinner without depending on an unloaded
  aggregate component stylesheet.
- Keep reduced-motion behavior explicit and accessible.
- Place the Search Knowledge results scrollbar flush against the pane's right edge while
  preserving the existing content inset.
- Render source-position links for the production nested span provenance shape.
- Preserve direct and exact provenance labels and exact source navigation.

## Agent Notes

- 2026-09-09 Copilot: Reproduced the static spinner and confirmed that no circular-progress
  animation rules or keyframes are loaded. Measured the results scrollport 13 px inside the
  section's right edge. The retained index contains 2,898 exact PDF chunks and 106 nested span
  chunks; the latter are the chunks currently missing page links.
- 2026-09-09 Copilot: Replaced the unloaded component styling dependency with a local,
  accessible 0.8-second spinner animation and an explicit reduced-motion state. Browser-computed
  transforms changed during the animation.
- 2026-09-09 Copilot: Moved the results section's right inset inside the scrolling element. The
  live scrollport and section now share the same right edge while result content retains 13 px of
  breathing room.
- 2026-09-09 Copilot: Added production nested-span decoding and rendered-source regressions, so
  all retained PDF chunks expose their indexed page or page range. Verified 168 focused frontend
  tests, frontend typechecking, repository Rust/Biome lint, patch checks, and the browser geometry
  and animation. The Impeccable detector reported only two unrelated pre-existing theme warnings.
