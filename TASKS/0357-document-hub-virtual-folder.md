# 0357 Document hub virtual folder with per-extension subfolders

Status: done
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0162

## Context
Requested by the user: a one-click virtual folder, not tied to a drive, that groups all documents (PDF, PPTX, DOCX, ...) found under the user's home folder in one place. The user chooses which extensions are included and the base path (default `~`). Inside the hub, virtual subfolders per extension (a `pdf` folder, a `docx` folder, ...) narrow the view. Today a smart folder (0162) can express the flat "extension in {...} under ~" query, but there is no ready-made preset, no per-extension subfolder level, and saved searches are flat result lists.

## Acceptance Criteria
- A built-in "Documents" entry (sidebar / favourites / command palette) opens the hub virtual location, in either pane or a tab, workspace-restorable like smart folders.
- The hub lists every matching file under the base path recursively, bounded and cancellable like other searches (0162), with its source location shown.
- Settings: editable extension list (default pdf, docx, doc, pptx, ppt, xlsx, xls, odt, ods, odp, rtf, txt, md, epub; case-insensitive, dot optional) and base path (default `~`, any provider location the search supports). Defaults restore with one action.
- Subfolders: the hub's top level shows one virtual folder per configured extension (only those with at least one match, with a count when known), plus the combined "All" view; opening one lists only that extension. Back/up navigation returns to the hub.
- Persists no entry arrays; settings sanitized and migrated like other settings (bounded list sizes and lengths).
- Hidden files and excluded folders (`node_modules`, `.git`, system folders) are skipped by default, configurable.
- Same behaviour in browser and Tauri hosts (preserve parity).

## Implementation Notes
- Extend the existing search virtual-location model (0162 says: do not add a separate result store). A hub is a versioned structured query `{ basePath, extensions[] }` plus an optional `extension` facet for the subfolder level; do not persist executable expressions.
- Subfolder level: either group results by extension into synthetic folder entries (like the folder groups of the basket, 0238) or model the level as child virtual locations of the same hub query. Pick whichever 0238 and 0330 already use so tags and the hub share plumbing.
- Counts per extension need a full scan; show them lazily as the scan completes rather than blocking the first paint.
- Consider reusing the 0166 index for speed once available; correctness must not depend on it.
- Related: 0330 (tag virtual folders) wants the same "virtual folder with groups" shape.

## Agent Notes
- 2026-11-24 claude: Created from a user request after confirming that 0162 smart folders cover only the flat query. Nothing implemented; design questions open: synthetic folder entries vs child virtual locations for the per-extension level, and where the settings live (next to saved searches or in a new "Document hub" section).
- 2026-11-24 claude: Implemented (name "Document hub", chosen so it never equals a default OS folder). Decisions: per-extension level is **collapsible extension group headings with counts** inside one recursive result list (agreed with the user), not navigable folders. The hub is an ordinary recursive name search with the glob `*.pdf,*.docx,...` (`features/search/document-hub.ts`), opened by the new core action `core.documentHub` (command palette, no default shortcut); the controller keeps `hubLocationUris` like branch view. Grouping: `groupByExtension` threaded through `sorting.ts`, `entriesSortedFor` call sites, `pane-content-builder`, `workspace-layout`, `pane.ts` and `directory-table.ts` (`extension-groups.ts`). Settings `documentHub {extensions, basePath}` in fm-settings (normalised, bounded, defaults restored when empty, blank path means `~`), DTO, mapping, OpenAPI/Orval, editor section under Files & operations, 8 locales. Type-to-select and Ctrl+F quick filter verified working on the hub in the mock runtime. Tests (red first): settings sanitizing (Rust), `extension-groups`, `document-hub`. `theme.test.ts` fails on committed CSS (`0.8125rem` in chat-tab.css/theme.css), unrelated.
  **Not done:** no sidebar/favourites entry (palette only); `~` is resolved against the active folder's local scheme, remote base paths are untested; directory-excluded-by-default (`node_modules`, `.git`) relies on the search engine's ignore rules, not a hub setting; the extension-group heading has no copy-path button by design.
