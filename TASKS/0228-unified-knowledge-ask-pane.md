# 0228 Unified Knowledge Ask pane

Status: done
Priority: high
Subsystem: frontend, rag
Depends on: 0186, 0207, 0209

## Context

Grounded Ask still opens as a modal even though Structured Knowledge Search already has a
first-class transient pane with hybrid retrieval, inspectable evidence, source navigation, and
optional answer generation. The modal interrupts the dual-pane workflow and duplicates retrieval,
answer, citation, and source-opening concepts.

## Acceptance Criteria

- The Ask action opens the existing transient Knowledge tab in an explicit Ask mode instead of a
  modal; Search Knowledge continues to open the same surface in Search mode.
- Search and Ask share one query, scope, retrieval plan, evidence set, and source-navigation path.
  Ask generation consumes the displayed evidence without rerunning retrieval.
- Ask mode makes question/answer the primary workflow and presents references and discovered
  evidence in a distinct aligned region. At usable desktop pane widths the two regions form a
  balanced split; at narrow widths they stack without clipping or misaligned controls.
- Opening Knowledge or Ask does not automatically replace the opposite file pane. Opening a source
  continues to use the opposite pane explicitly.
- Loading, empty, insufficient-evidence, generation, cancellation, error, keyboard, screen-reader,
  localization, and no-profile states remain understandable.
- The legacy Ask modal is no longer mounted or opened by shell actions. Existing backend contracts
  remain available and no generated transport files are hand-edited.
- Focused tests cover Ask-mode tab opening, Search-mode compatibility, evidence/answer layout, source
  navigation, and narrow-layout semantics.

## Implementation Notes

- Reuse `KnowledgeSearchPane` and the existing transient-tab lifecycle from 0209.
- Preserve the pane-based file-manager topology; do not silently consume both workspace panes.
- Keep the compact minimal design system: rectilinear regions, 1px separators, aligned control
  rows, and no nested scroll containers.

## Agent Notes

- 2026-09-23 Copilot: Created from the decision to replace the modal Ask flow with one unified
  Knowledge pane. The existing 0207 answer path is the canonical downstream generation seam; 0186's
  modal remains useful only as migration evidence and must not remain shell-mounted.
- 2026-09-23 Copilot: Routed Search and Ask through one transient Knowledge pane, removed the
  shell-mounted modal and its controller state, and made Ask retrieve once before generating from
  the displayed evidence fingerprint. Added a balanced answer/source split that stacks from the
  pane's own container width, preserving the opposite file pane for explicit source navigation.
- 2026-09-23 Copilot: Verified light-theme cursor descendants inherit the high-contrast white
  foreground on the dark-blue cursor, including icons and secondary metadata.
- 2026-09-23 Copilot: Validation passed: 2,166 frontend tests, focused Knowledge/AppShell/theme
  tests, TypeScript, repository Rust lint, and desktop/narrow browser inspection. Biome reported
  only the repository's existing CSS specificity warnings and schema-version notice after changed
  files were formatted.
