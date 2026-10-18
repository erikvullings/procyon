# 0142 Plugin-contributed preview renderers

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: cross-cutting
Depends on: 0071, 0053

## Context

Split out of [0071](0071-file-preview-architecture.md) in the 2026-08-15 re-triage, when that
task's built-in renderers (text, image, audio, PDF, comic, EPUB, metadata) landed but this
extensibility piece didn't. The F3 viewer's renderer registry
(`frontend/src/features/preview/content-preview.ts`'s `resolvePreviewKind`/`PreviewKind`) is
currently a closed set of hardcoded content kinds - task 0071 always intended it to be "additive"
so new kinds could be registered without a rewrite, but nothing has ever plugged in from outside
the core app. This task is what actually lets a plugin (task 0053's plugin system) contribute one.

## Acceptance Criteria

- A plugin manifest can declare a preview contribution: which file extensions/MIME types it wants
  to render, and what it returns (plain text/HTML to render sandboxed - mirroring the existing
  Markdown preview's DOMPurify-sanitized-HTML approach - not arbitrary script execution; task
  0071's "previewed files are never executed" invariant must hold for plugin previews too).
- When a plugin's declared extension matches an F3-opened file and no built-in renderer already
  claims that extension, the viewer calls into the plugin runtime (task 0054) to render it, with
  the same error-isolation guarantees other plugin contribution points already have (a failing
  preview plugin shows a friendly "this preview failed" state, never crashes the viewer).
- Built-in renderers always win over a plugin's for the same extension (no accidental shadowing of
  text/image/PDF/etc.) - a plugin can only fill genuinely unhandled extensions, or explicitly
  opt to extend the "unsupported" fallback.
- A sample plugin (mirroring 0055/0056's "sample plugin" precedent) demonstrating the contribution
  point end-to-end - e.g. a simple syntax-aware preview for a niche format not already covered.
- Tests: manifest parsing for the new contribution type, renderer dispatch (built-in vs. plugin,
  precedence), sandboxed HTML rendering (no script execution even if a malicious/buggy plugin
  tries), plugin failure isolation.

## Implementation Notes

- Reuse the plugin manifest/discovery/permission model from 0053 rather than inventing a parallel
  one - a preview contribution is just a new `contributions` entry type alongside the existing
  `actions`/columns kinds already supported (see `crates/fm-plugin-api`).
- The sanitization approach already exists for Markdown
  (`frontend/src/features/preview/markdown-preview.ts`'s `safeMarkdownHtml`,
  `FORBID_TAGS`/`FORBID_ATTR`/`ALLOWED_URI_REGEXP`) and was reused again for EPUB chapters
  (`epub-preview.ts`'s `sanitizeEpubChapterHtml`) - reuse the same DOMPurify configuration a third
  time here rather than a new one.

## Agent Notes

- 2026-10-18: Reviewed Total Commander Lister (WLX) plugins with the user. Decisions: no syntax
  plugin (CodeMirror 6 already highlights F3 text via `editor/editor-language.ts`; instead added the
  missing popular languages there); CSV/TSV/Excel/JSON already built in via 0100/0172; hex is a
  **builtin** (done: `preview/hex-preview.ts`, `hex` viewer content kind, reuses 64 KiB text paging)
  and replaces the old "unsupported" fallback for binary files; SQLite split to 0251 (builtin
  recommended). Planned separate, on-by-default preview plugins once the contribution point exists:
  `preview-torrent`, `preview-eml`, `preview-vcard-ical`. Contribution point itself (manifest
  `contributions.preview`, render hook, bounded read permission; Lua vs JS undecided, lean Lua since
  0226 is open) is NOT started.
- 2026-10-18: Implemented end to end. API: `contributions.preview` (`PreviewContribution`, Lua only,
  needs `selected_entry_content_read`) in `fm-plugin-api`; `PluginRuntime::render_preview` in
  `fm-plugin-runtime` (fresh VM, no host table, 1 s / 32 MiB / 20M-instruction / 512 KiB budgets,
  same failure counting and auto-disable). Host: `PluginManager::render_plugin_preview` +
  `FileManagerService::render_plugin_preview` (reads <=512 KiB via the VFS), `POST
  /api/v1/plugins/{pluginId}/preview`, Tauri `render_plugin_preview`, `PluginDescriptorDto.preview`.
  The render call is synchronous but bounded (<= 1 s), like action invocation.
  Frontend: `PluginPreviewResult`, `renderPluginPreview` on all three clients, `plugin-preview.ts`
  (routing with built-in precedence + sanitizer: Markdown/EPUB DOMPurify options plus no
  `img/video/link/...`), `pluginPreview`/`pluginPreviewFailed` viewer content. Bundled plugins
  `plugins/preview-{torrent,eml,vcard-ical}`, tested through the real runtime in
  `crates/fm-plugin-runtime/tests/bundled_previews.rs` (escaping, truncation, invalid UTF-8,
  nesting bombs, budget). **Enabled by default** via settings schema v9: new installs get
  `DEFAULT_ENABLED_PLUGINS`, upgrades seed them once, and a v9 user's disable is respected.
  Decisions: no syntax plugin (CodeMirror already highlights; 25 languages added to
  `editor-language.ts`); CSV/TSV/Excel/JSON already built in (0100/0172); hex is a builtin
  (`hex-preview.ts`); SQLite is 0251 (builtin recommended). Not done: the dev mock runtime has no
  preview fixture plugin; the F3 "Alt+Space" metadata panel shows nothing for plugin previews;
  executable/font previews were deferred.
- 2026-10-18 (code review): fixed sanitizer gaps (form controls, `background`/`src`/`href` stripped),
  plugin failure now falls back to text/hex instead of a dead-end, preview errors are logged but no
  longer count toward auto-disable (untrusted input), claim check happens before file I/O, URI file
  names are percent-decoded, EML/ICS plugins cap output and handle RFC 2047 word joining / nested
  VALARM, torrent plugin validates numbers. The Lua call still runs synchronously on the async
  worker (bounded to 1 s) - revisit with `spawn_blocking` if `PluginManager` becomes `Arc`-shared.
