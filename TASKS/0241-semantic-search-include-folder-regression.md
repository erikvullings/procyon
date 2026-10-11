# 0241 Restore Include folder in semantic search

Status: done
Priority: high
Subsystem: frontend
Depends on: 0179, 0183

## Context

User repro: "When opening the semantic search pane, I can no longer include the folder for indexing." The expected behavior is a separate, confirmed **Include folder** action, as established by 0179 and 0183. Actual behavior: the action is missing from the opened pane. The precise cause and affected platforms were initially unknown. Current inspection finds a one-way checkbox nested behind the search pane's Settings menu rather than a visible action on the pane.

## Acceptance Criteria

- An unindexed current folder offers a discoverable Include folder action directly in the semantic search pane; keyboard activation opens the existing consent/preview flow.
- No folder is indexed or consented merely by opening the pane, changing scope, or clicking the action without confirmation.
- Indexed folders do not offer redundant inclusion; browser and Tauri use the same client-driven UI.
- Focused frontend tests exercise the action, consent boundary, and unavailable/already-included state.
- Treat as a Gemma-release blocker until fixed and dispositioned; production approval remains noGo.

## Implementation Notes

- `frontend/src/features/semantic/knowledge-search-dialog.ts` currently renders the one-way control only within the Settings modal and uses `SemanticFolderEnrolmentPrompt` for confirmation.
- Preserve the existing client boundary and provider-aware folder status; do not silently enrol any path.

## Agent Notes

- 2026-10-10: Began investigation on PR #115 after direct user report. Existing `knowledge-search-dialog.test.ts` only verifies the checkbox inside Settings; extend coverage for the pane's visible action and keyboard behavior.
- 2026-10-10: Restored a visible, focusable Include folder button in `frontend/src/features/semantic/knowledge-search-dialog.ts`, replacing the undiscoverable one-way checkbox hidden in the pane Settings modal. It opens the existing `SemanticFolderEnrolmentPrompt`; no confirmation happens on click. Updated focused and app-shell tests to check absent/already-included folders, the explicit confirmation boundary, and the host-independent client path. The exact production Gemma approval is unchanged.
