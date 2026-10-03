# 0235 Search then Ask in one Knowledge tab

Status: done
Priority: high
Subsystem: frontend, rag
Depends on: 0207, 0228

## Context

The installed v0.3.1 Ask mode uses the search subject field as the question, splits even a narrow
pane between answer and results, renders the model's `{"answer":"..."}` wrapper as prose, and
prevents selecting answer text in Tauri. The user wants one Knowledge tab: search first, then ask
a separate question about the returned evidence; switch between full-width Results and Answer
without replacing the search query or silently rerunning retrieval.

## Acceptance Criteria

- A Knowledge tab opens in Search mode. After a successful search, Ask exposes a separate,
  clearly labelled question field; Search keeps its original subject, scope, options, and results.
- Answering uses the displayed evidence fingerprint without another retrieval. Results and
  Answer occupy the pane independently, not side by side; switching back preserves both.
- The answer-only question reaches the generation prompt as a bounded field, never retrieval
  planning. Changing the question invalidates an old answer and cancels its in-flight generation.
- The shared grounded JSON output contract is decoded before display/citation resolution;
  malformed JSON is an explicit generation failure. Answer prose is selectable in Tauri.
- Both browser and desktop adapters preserve the typed request and all existing cancellation,
  authorization, citation, accessibility, and localization behavior.

## Implementation Notes

- Preserve the Search-first, evidence-bound architecture in 0207/0228. The old Ask command may
  focus an existing Knowledge tab but must not create a parallel answer surface.
- Generated HTTP/OpenAPI clients are regenerated, not hand-edited.

## Agent Notes

- 2026-10-03 Copilot: User supplied a screenshot of a Dutch answer shown as a literal JSON object,
  confirmed answer text could not be copied, and chose the single-tab Search -> Ask workflow.
  The existing knowledge answer path reuses `grounded_system_prompt`, which requests JSON, but
  unlike the RAG path it returns the model string without decoding it. The global no-selection
  CSS also overrides the answer container's rule on descendants. Work is in progress.
- 2026-10-03 Copilot: Implemented Search-first entry and one reusable Knowledge tab; the Ask
  switch becomes available after a completed search, opens a separate bounded question field,
  and uses the retained evidence fingerprint. Results and Answer share the pane at full width.
  Editing a question cancels/invalidates the previous answer without changing the results.
  The backend decodes the shared JSON answer contract before resolving citations; the answer
  descendants explicitly restore text selection including WebKit. Regenerated the typed API and
  the non-production Knowledge NO-GO evaluation report against the changed request source.
  Verified the pane/shell tests, Knowledge Rust tests, frontend build and repository lint;
  Chromium computed selection is `text` on answer descendants and the hidden results section
  leaves a single-column answer. Public installer first-launch index visibility and the Finder
  cursor belong to 0198 and 0062, not this task.
