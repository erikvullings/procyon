# 0265 Duplicate and near-duplicate music finder

Status: done
Priority: low
Subsystem: backend, frontend
Depends on: 0077

## Context
Checksum duplicates (0077) miss the same track in different formats or bitrates. Combine tag metadata (artist, title, album, duration) with checksums to find likely duplicates.

## Acceptance Criteria
- Scan a folder tree for audio files; group exact duplicates (checksum) and likely duplicates (normalized artist+title+duration within tolerance) with a confidence label.
- Results open as a virtual folder with keep/remove helpers (prefer higher bitrate or lossless); deletion uses the normal operation engine and Trash.
- Cancellable, progress-reporting job; unreadable tags degrade gracefully.

## Implementation Notes
- Reuse `fm-metadata` for tags; related to perceptual image duplicates (0170) — share the grouping UI if possible.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Backend: fm-metadata::audio (lofty 0.25; tags + duration/bitrate/lossless, local files only - lofty needs seekable files so remote/archive audio degrades to exact-match-only and is counted in unreadableTags). fm-application/music_duplicates.rs: job (list via fm_checksum::collect_candidates, exact groups via staged checksum detector, tag read with spawn_blocking x4, likely groups = same normalised artist+title with durations within 3 s, keeper = lossless > bitrate > size) with pure group_music tests + end-to-end test; REST POST/GET/cancel /api/v1/music-duplicate-scans (roots checked) + Tauri commands + client methods on http/tauri/mock. Frontend: features/music-duplicates (controller polls progress by phase, panel with confidence badges, 'select suggested removals', last-copy guard, removal via OperationsController.trash = normal operation engine/Trash), palette action client.findMusicDuplicates (selected folders or active folder). Deviation: results open in the existing inline duplicate-review style panel (shared CSS) rather than a virtual folder. Scan progress is polled, not pushed over SSE.
