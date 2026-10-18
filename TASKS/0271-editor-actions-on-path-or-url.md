# 0271 Editor actions on a path or URL under the cursor

Status: open
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
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
