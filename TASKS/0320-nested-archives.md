# 0320 Archives inside archives (browse, read, edit with confirmation)

Status: done
Priority: medium
Subsystem: backend + frontend
Depends on: 0076

## Context
Opening a zip inside a zip failed with "Invalid 'uri' parameter": `archive://` only accepted a
local file before the `!`. Total Commander behaviour is the model (inner archive opens as a
folder; editing it asks before the outer archive is rewritten).

## Acceptance Criteria
- `archive://outer.zip!/dir/inner.zip!/file` parses, lists, reads (incl. byte ranges), and the
  UI opens an archive entry as a folder; `..` from a nested root lands in the folder holding it.
- A stored (uncompressed) unencrypted inner ZIP is read in place through a byte window of the
  outer file (no copy, no cache cap); anything else (deflated, 7z, tar, rar) is expanded once into
  the session extraction cache (512 MB cap).
- ZIP nested one level deep is editable: changes go to a temp working copy and stay pending.
  Nothing touches the outer archive until the user confirms; the dialog offers Save / Discard /
  Not now. Save replaces the entry (stored) in the outer zip; refused if the outer file changed.
- Deeper levels stay read-only; extract action stays local-only.

## Implementation Notes
- `fm-domain` `ParsedArchiveUri` has `layers`; `fm-archive`: `ByteWindow`/`WindowReader`/`ZipInput`,
  `open_nested`, `NestedEdit`, `pending_edits`/`commit_edit`/`discard_edit`.
- API: `GET /api/v1/archives/edits` (`listArchiveEdits`), `POST /api/v1/archives/edits/resolve`
  (`resolveArchiveEdit`); Tauri `list_archive_edits`/`resolve_archive_edit`.
- Frontend: `archive-location.ts`, `navigation.ts` (`parentLocation`), `archive-edit-dialog.ts`,
  `reviewArchiveEdits` in `app-shell.ts` (runs after each navigation, asks once no tab shows the
  nested archive).

## Agent Notes
- 2026-11-10 claude: built and tested (Rust: 5 new nested tests in `archive_provider.rs`;
  frontend: navigation, archive-location, dialog tests). Known limits: unsaved nested edits are
  lost silently when the app quits (no quit-time prompt); a nested archive needs a stored inner
  ZIP for zero-copy; encrypted outer entries are not editable; panes showing the outer folder are
  not refreshed explicitly after a save (relies on the file watcher). Not verified in a real window.
