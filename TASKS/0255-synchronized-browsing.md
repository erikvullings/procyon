# 0255 Synchronized browsing

Status: open
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
