# 0262 Audio queue and persistent mini-player

Status: done
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
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Queue state is AppState.player (state/model.ts, playerPatch) with pure transitions in features/player/player-state.ts (queue, shuffle order, repeat off/all/one, unavailable-track skipping). player-controller.ts drives one <audio> via client.mediaUrl (0260 streaming), applies seek/volume, and registers Media Session metadata/handlers (play, pause, prev, next, seekto) when navigator.mediaSession exists. MiniPlayer (mini-player.ts, new shell grid row above the F-key bar) persists across navigation/tabs: transport, seek, volume/mute, shuffle, repeat, expandable queue (jump/remove), 'N tracks skipped' note. Playlists (0261) play via Play all / Add to queue buttons and row double-click (viewer attr onPlayTracks); missing/network entries are not queued. Shortcuts: frontend client actions client.playerTogglePlay/Next/Previous (palette + user-bindable, no default chords to avoid clashes), dispatched through new generic invokeClientAction in the keydown handler and action-command controller. Not done/untested: gapless playback (HTMLAudioElement cannot guarantee it; documented limit), Media Session behaviour on WKWebView/WebView2/WebKitGTK, audio-only (video tracks are not queued).
