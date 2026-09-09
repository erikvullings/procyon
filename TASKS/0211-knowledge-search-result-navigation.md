# 0211 Knowledge search result navigation

Status: done
Priority: high
Subsystem: frontend
Depends on: 0210

## Context

Live testing of the document-oriented Search Knowledge pane exposed remaining layout and
navigation gaps. PDF evidence opens at the right page but its fit calculation includes the
preview container's padding, causing horizontal and vertical overflow while the toolbar labels
the fitted scale as `100%`. Knowledge-need choices still render bordered pills, the subject
composer lacks a visible Search action, and the source count is detached from its heading.

Document groups are structurally correct but visually run together. Evidence rows already carry
indexed section paths and exact page/line/slide provenance, yet that structure is not shown and
only the document heading can navigate to a source position.

## Acceptance Criteria

- Fit PDF pages to the preview content box so a fitted page does not cause scrollbars, and label
  the default PDF scale as `Fit`.
- Remove borders from both the knowledge-needs fieldset and its visible checkbox labels while
  preserving checked and keyboard-focus states.
- Add a visible Search button beside the Subject editor without changing Enter and Shift+Enter
  behavior.
- Render the Sources heading and document/section count on one line.
- Present document groups as a numbered list with a clear visual boundary per document.
- Show each evidence row's indexed section path and structural position. Where no section title
  was indexed, use the structural position as the honest label.
- Make every evidence section's position control open that exact source page, slide, line range,
  cell range, or block.
- Add focused regressions for PDF fit bounds, controls, result hierarchy, and per-section source
  navigation.

## Implementation Notes

- PDF text-layer conversion preserves paragraph blocks and page provenance but does not have
  reliable font/heading metadata. Do not infer headings from capitalization or the first line.
- Keep document ranking and in-document source ordering from 0210 unchanged.
- Reuse `knowledgeProvenanceLabel` and the existing `onOpenSource` path so page navigation remains
  provider-neutral and authorized at activation time.

## Agent Notes

- 2026-09-09 Copilot: Created from live testing of task 0210. Confirmed that PDF fit receives the
  padded scroll container's `clientWidth`/`clientHeight`, which makes the fitted canvas plus
  padding overflow. Confirmed that `sectionPath` and provenance are already present on every
  evidence DTO; PDF section paths are currently empty because the converter extracts text blocks
  without typography.
- 2026-09-09 Copilot: `file-viewer.ts` now fits PDF canvases to the scroll region's content box and
  labels the default PDF scale as `Fit`. `knowledge-search-dialog.ts` now provides a visible Search
  action, a one-line source count, borderless need labels, numbered document groups, and an exact
  source-navigation control for every evidence section. Indexed section paths are shown when
  available; otherwise the page, slide, line, cell, or block provenance is the honest section
  label.
- 2026-09-09 Copilot: The live mock pane measured one vertical scroll owner and no horizontal
  overflow at a two-pane desktop width. Focused component, shell-integration, theme, type, and
  repository lint gates passed.
