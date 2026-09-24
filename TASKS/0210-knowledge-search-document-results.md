# 0210 Knowledge search document results

Status: done
Priority: high
Subsystem: frontend, backend
Depends on: 0209

## Context

Live testing of the pane-first Search Knowledge UI exposed a second usability pass and one
authorization mismatch. The composer still has a redundant Search button and bordered needs
container; completed results can shrink over the tab strip and introduce horizontal scrolling.
Evidence is presented as ranked chunks with diagnostic metadata rather than as readable,
document-oriented content. Encoded filenames remain visible.

Desktop whole-library search can return evidence indexed through a retained historical workspace
tenant, but source activation resolves only against the currently open workspace. Clicking such a
valid result therefore reports that the source could not be opened.

## Acceptance Criteria

- Submit from the subject editor with Enter and preserve Shift+Enter for a newline; remove the
  explicit Search button.
- Show only a compact, accessible spinner while searching and preserve cancellation on close.
- Remove the outer border around the knowledge-need choices.
- Keep Search Knowledge below the tab strip and prevent horizontal scrolling at every result size.
- Decode URI-encoded document titles and truncate each title to one line.
- Always group evidence by document. Rank documents by their best search relevance, sort each
  document's evidence by source position, and render the retained content as sanitized Markdown.
- Make the document title the source-opening control and remove per-chunk subject/need/rank
  diagnostic lines from the default result surface. Show each section's fused relevance as a
  rank-coloured indicator beside its page or structural source reference, with a localized
  qualitative label available on hover and keyboard focus instead of exposing the implementation-
  specific reciprocal-rank-fusion score.
- Resolve host-wide whole-library evidence through any currently authorized retained workspace
  scope while keeping server callers restricted to their authenticated workspace.
- Refresh displayed retrieval capabilities from each completed search so stale startup capability
  warnings do not contradict the route that produced the result.
- Add regressions for document ranking/order, encoded titles, keyboard submission, pane overflow,
  and cross-workspace source activation.

## Implementation Notes

- Keep the backend's ranked evidence DTO intact; the document projection is a frontend
  presentation over `documentId`, `finalRank`, `fusedScore`, and `sourcePosition`.
- Continue sanitizing rendered Markdown through the existing `safeMarkdownHtml` helper.
- Authorization must be revalidated at activation time; do not trust indexed paths or bypass
  consent checks.

## Agent Notes

- 2026-09-09 Copilot: Created from live testing screenshots and source-opening failures after task
  0209.
- 2026-09-09 Copilot: Confirmed the activation mismatch with a failing regression: host-wide
  whole-library retrieval deliberately spans retained workspace tenants while source resolution
  checked only the active workspace. Host activation now reauthorizes against the occurrence's
  retained workspace scopes; server callers remain restricted to their requested workspace.
- 2026-09-09 Copilot: Search results now rank documents by their best evidence, restore sections to
  document order, render every section as sanitized Markdown, decode and truncate source titles,
  and open the selected document in the opposite pane. The composer submits on Enter, preserves
  Shift+Enter for newlines, reports progress with a reduced-motion-safe spinner, and contains
  overflow below the fixed tab strip.
