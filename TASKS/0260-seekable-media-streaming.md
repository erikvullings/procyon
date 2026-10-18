# 0260 Seekable media streaming for the viewer

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0088, 0150

## Context
Audio is played in the F3 viewer by loading the whole file into memory as a base64 `data:` URI (`readFullAudioDataUri` in `frontend/src/features/preview/content-preview.ts`), and video (0150) needs a size cap for the same reason. That blocks long podcasts, lossless files, queues, and any remote (SFTP/S3/OneDrive) media. A seekable byte-range source would fix all of them.

## Acceptance Criteria
- Backend serves byte ranges of any VFS entry (local, archive, remote) to the webview with bounded memory, cancellation, and correct `Content-Range`/`Accept-Ranges` semantics.
- Works in both hosts: Axum route and Tauri custom protocol or equivalent, behind `FileManagerClient` (spec rule 9); no direct fetch from components.
- `<audio>`/`<video>` can seek in files of several GB without loading them whole; the `data:` URI path is removed or kept only as a small-file fallback.
- Authorization/path rules match existing read endpoints; tests cover range edge cases (open-ended, suffix, out of range).

## Implementation Notes
- Reuse `readFileRange` provider plumbing (0088) rather than a second read path.
- Tauri: custom URI scheme handler is the usual way to feed media elements; verify seeking on macOS WKWebView, Windows WebView2 and Linux WebKitGTK.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
