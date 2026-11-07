# 0318 F3 viewer: HTML preview and raw-source toggle

Status: done
Priority: low
Subsystem: frontend
Depends on: 0088

## Context
F3 showed `.html` files as raw, unwrapped text, and rendered Markdown with no way back to the
source. Wanted: HTML previewed as a page, and a toggle to the raw text (syntax highlighted, word
wrapped) for HTML and Markdown. GeoJSON already had a map/raw toggle (`toggleGeoRaw`).

## Acceptance Criteria
- `.html`/`.htm` open as a rendered preview; a Preview/Source toggle switches to read-only
  CodeMirror source with syntax highlighting and word wrap.
- Markdown gets the same toggle (rendered by default).
- The preview never runs scripts or loads anything over the network.
- Raw text in the viewer wraps long lines.

## Implementation Notes
- `frontend/src/features/preview/html-preview.ts`: `sandboxedHtmlDocument` (restrictive CSP meta
  prepended) and `isHtmlPreviewExtension`. The iframe has an empty `sandbox` attribute.
- `file-viewer.ts`: `TextContent` component (per-entry mode, resets when the entry changes);
  `EditorView.lineWrapping` for every viewer text body.

## Agent Notes
- 2026-11-07 claude: built as described. i18n (`viewer.textMode*`, `htmlPreviewTitle`) in all 8
  locales. Verified: 3 viewer tests + 3 html-preview tests, full vitest, tsc, biome. Limits: the
  preview shows only the loaded text window of a very large file; relative resources (images,
  stylesheets) and links do not work, by design (no base URL, CSP `default-src 'none'`, no popups);
  dark themes keep a white page canvas. Not checked visually in a real browser/Tauri webview.
  GeoJSON already had the raw toggle, so it was not changed; it now wraps in raw mode.
