# 0142 Plugin-contributed preview renderers

Status: open
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

- (none yet)
