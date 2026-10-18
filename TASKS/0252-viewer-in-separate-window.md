# 0252 Open the F3 viewer in a separate window

Status: done
Priority: medium
Subsystem: frontend, desktop
Depends on: 0088

## Context

Total Commander's Lister is a separate window. Procyon's F3 viewer is a tab in the opposite pane. Requested:
support a separate window as a setting.

## Acceptance Criteria

- Settings > File behaviour has "Open viewer (F3) in a separate window" (default off), persisted in
  `Settings.viewer_in_separate_window` / `viewerInSeparateWindow`.
- With it on, F3 opens an OS window on desktop and a popup in the browser host, showing the same viewer
  (controller + `FileViewer`), with theme, language and preview plugins applied.
- If the window cannot be opened (blocked popup, command failure) the in-pane viewer is used.
- Esc (outside inputs) closes the window; Ctrl/Cmd+F toggles search; Alt+Space toggles the info panel.
- Opens that depend on in-pane state (knowledge sources, Alt+Space preset) stay in a pane tab.

## Implementation Notes

- Window request = base64url JSON (`features/preview/viewer-window.ts`) in `?viewer=`; `main.ts` mounts
  `app/viewer-window.ts` instead of `AppShell` when it decodes. `fileViewerAttrs` (shared with the in-pane tab)
  wires controller to component.
- Desktop: Tauri command `open_viewer_window(payload, title)` validates the payload alphabet and size, builds a
  `viewer-<uuid>` window on `index.html?viewer=...`. `viewer-*` was added to `capabilities/default.json` and to
  `trusted_invoke_label` (UUID required); window-state frames are shared under one `viewer` key.
- Browser/HTTP: `window.open` of the same page; `sessionStorage` token is copied to the popup.

## Agent Notes

- 2026-10-18: Implemented. Tests: `viewer-window.test.ts` (codec, fallback), `app/viewer-window.test.ts`
  (component, Esc), `app-shell.test.ts` (setting + fallback), Rust `viewer_payload_accepts_only_bounded_base64url`
  and the extended trust-label/capability tests.
  Not verified by hand: the real Tauri window (macOS/Windows/Linux), popup behaviour in browsers, window-state
  restore. Limits: no "Open externally"/"Quick Look" difference from the pane viewer except they invoke actions
  without a pane id; no document-summary button or Tauri window-background sync in the separate window; F3 does
  not toggle an already open separate window closed.
- 2026-10-18 (code review): token now carries a slimmed entry (8 KiB cap); window `pagehide` disposes backend
  sessions. Left as is: the window re-implements a subset of `applyAppearance` (no icon theme, date/size format
  or native background sync) - extract a shared helper if that matters.
