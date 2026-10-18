# 0271 Editor actions on a path or URL under the cursor

Status: done
Priority: low
Subsystem: frontend
Depends on: 0099

## Context
In the in-app editor and viewer, paths and URLs in text (logs, Markdown, config files) should be actionable.

## Acceptance Criteria
- With the cursor on a path or URL: 'Open in pane' / 'Reveal' for local and VFS paths (relative to the file), 'Open URL' via the system opener for http(s) only.
- Path detection tolerates `file:` URIs, `~`, `path:line:col` suffixes (jump to line in the editor).
- No arbitrary execution: only navigation or opening via existing safe actions.

## Implementation Notes
- Implement as CodeMirror extension plus actions through the registry (0049).

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Pure detection in features/editor/link-detection.ts (http(s) URLs with trailing-punctuation/bracket handling; absolute, ./ ../ ~/ , C:\ , file: URIs, bare relative paths only when they look like files; path:line[:col] suffixes), resolution in link-actions.ts (relative to the edited file via the playlist resolver, ~ via getHomeDirectory, existence + kind via resolveLinkTarget, only http/https reach the system opener). LinkAwareEditor wraps CodeMirror (contextmenu + cursor hooks) in the F4 editor and the F3 text viewer: right-click a path/URL for Open URL / Open in pane / Reveal in file manager (local files) / Edit file [at line N] (editor jumps to and selects the line). Open-in-pane targets the other pane (cursor lands on the file) or the same pane when only one exists; unsaved editor changes are never dropped (toast asks to save first). Palette actions client.openLinkAtCursor / client.revealLinkAtCursor act on the caret position of the active pane's editor. Nothing is executed: only navigation, existing reveal action and openExternalUrl. Deviation: actions use the existing client.* mechanism rather than Rust-registry descriptors; context-menu is the primary UI (no hover underline/ctrl-click yet). Not exercised in a real WebView (CodeMirror coordinate lookup is guarded for jsdom).
