# 0209 Knowledge search pane UX

Status: done
Priority: high
Subsystem: frontend
Depends on: 0206

## Context

The shipped Search Knowledge modal wastes pane-sized space, introduces a redundant outer scrollbar,
and puts retrieval controls and parser diagnostics ahead of results. Search results can start below
the visible area. Source navigation also lacks the natural dual-pane workflow: keep search in one
pane while opening the exact source preview in the other pane.

## Acceptance Criteria

- Open Search Knowledge as a temporary tab in the active pane; do not restore that tab with a later
  workspace session.
- Close the temporary tab with normal tab affordances and preserve ordinary directory tabs.
- Open selected evidence in the opposite pane using the exact resolved source location and
  structural provenance, including the correct page when available.
- Make the default surface compact: subject, knowledge needs, primary Search action, status, and
  results stay visible without nested page scrolling.
- Remove Related terms and Interpretation from the default visual workflow.
- Put scope, retrieval mode, query language, query plan, trace, and result grouping under Advanced.
- Default result order to relevance and explain any remaining grouping choices in plain language.
- Preserve keyboard, screen-reader, cancellation, no-LLM, HTTP/Tauri/mock parity, and existing
  structured-query behavior.
- Add focused frontend regressions for temporary-tab lifecycle, simplified disclosure, and
  opposite-pane exact-source preview.

## Implementation Notes

- Reuse the existing pane/tab and viewer controllers rather than adding a second navigation model.
- The search surface may retain an internal canonical DSL and parser for backend equivalence, but
  parser terminology should not obstruct the common visual workflow.
- Keep advanced controls available for expert and diagnostic use.

## Agent Notes

- 2026-09-09 Copilot: Created from live usability feedback. The incumbent modal shows two
  scrollbars and places low-frequency query mechanics before results. The intended Operate-mode
  hierarchy is question -> needs -> search -> evidence, with expert controls disclosed on demand.
- 2026-09-09 Copilot: Completed the pane-first redesign using the existing transient-tab command,
  whose tabs are discarded at workspace startup. Search Knowledge now has one scroll owner, a
  compact question-and-needs surface, relevance-first results, and one Advanced disclosure for
  scope, retrieval, DSL, grouping, plan, trace, and coverage details. Related terms and
  interpretation are no longer part of the default visual workflow.
- 2026-09-09 Copilot: Evidence is resolved against current authorization and opened in the opposite
  pane. PDF page and presentation slide provenance select the corresponding preview page, including
  when reusing an already-open viewer. Added regressions for transient-tab lifecycle, disclosure,
  source resolution, opposite-pane routing, provenance parsing, and initial-page selection.
- 2026-09-09 Copilot: Frontend type checking and all 313 affected tests pass. Automated screen
  capture was unavailable on the host, so final visual inspection remains manual; the
  semantic-enabled Tauri process is running against the existing indexed database for that check.
