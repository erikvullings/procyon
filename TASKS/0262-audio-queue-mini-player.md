# 0262 Audio queue and persistent mini-player

Status: open
Priority: medium
Subsystem: frontend
Depends on: 0260, 0261

## Context
Playback is one file at a time inside the viewer and stops when the viewer closes. Playing a playlist needs a queue and a player that survives navigating folders.

## Acceptance Criteria
- Queue with play/pause, next/previous, seek, volume, shuffle, repeat; current track and progress shown in a mini-player that persists across navigation and tab switches.
- Playlist files (0261) can be played in order; unavailable tracks are skipped with a visible note.
- Media Session API integration (OS media keys, now-playing info) where the webview supports it.
- Queue state lives in the Meiosis tree (ADR 0007), not in component-local state; keyboard shortcuts configurable via the action registry.

## Implementation Notes
- Depends on streaming (0260) so large and remote files play without loading whole files.
- Gapless playback is a stretch goal; document the webview limits.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
