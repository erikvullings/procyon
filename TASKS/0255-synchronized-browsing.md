# 0255 Synchronized browsing

Status: done
Priority: low
Subsystem: frontend
Depends on: 0027, 0026

## Context
When enabled, navigating in one pane (enter folder, parent, back) mirrors the same relative move in the other pane. Useful for comparing two similar trees (backups, branches). Not present today.

## Acceptance Criteria
- Per-workspace toggle (action, shortcut, toolbar/menu state indicator, persisted in workspace state).
- Entering a subfolder in pane A enters the same-named subfolder in pane B if it exists; parent navigation mirrors; if the counterpart is missing, panes desynchronize visibly (indicator) without error dialogs and resync when paths realign.
- Works across providers (local/remote/archive) via the shared relative-path logic; no feature specific to one adapter.
- Unit tests for mirroring rules, including missing counterpart and case-insensitive volumes.

## Implementation Notes
- State lives in the Meiosis tree (ADR 0007); implement as an action reacting to navigation patches, not a local component hook.
- Compute relative path on `Location` segments, not strings.

## Agent Notes
- Created 2026-10-18 from the feature gap review. No work started.
- 2026-10-24 claude: Implemented. `frontend/src/features/navigation/synced-browsing.ts` wraps the NavigationController (navigate/parent/back/forward) and replays user moves in the other pane using Location segments (`locationPath`/`relativeSegments`, case-insensitive; local/sftp/archive alike). Paired anchors record the locations that corresponded at enable time; a move below an anchor maps to the same relative path below the other anchor (child resolved via `listDirectoryChildren`, keeping the counterpart's spelling); going above the anchor re-anchors and raises the other pane; a missing counterpart or unrelated jump leaves the other pane alone and sets state `desynced` (no dialogs); it resyncs when paths realign. New core action `core.syncBrowsing` (Ctrl+Alt+S; Rust registry, mock actions.json, keydown route, palette handler), per-pane toggle/indicator button in the breadcrumb row (off/synced/desynced, 8 languages). Verified: 17 tests in `synced-browsing.test.ts` (mirroring rules, missing counterpart, case-insensitive, cross-provider, persistence, interleaved moves) + keydown/palette tests; full frontend vitest and fm-application action tests pass; tsc clean. Known gap: the toggle is persisted per workspace id in browser localStorage (not the backend workspace/settings model, which would need a Rust+OpenAPI field); switching workspace does not re-anchor until the toggle is flipped; only the first two panes participate.
