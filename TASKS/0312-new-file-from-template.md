# 0312 New-file-from-template

Status: in_progress
Priority: low
Subsystem: frontend, backend
Depends on: none

## Context
Directory Opus/XYplorer power users create files from templates ("New TODO", "New Meeting Note",
"new.sh") straight from the context menu instead of opening an editor and hand-rolling boilerplate.
Procyon can already create files (0035/0036) and has a plugin system (0053) — a template layer is a
small, delightful addition.

## Acceptance Criteria
- A "New ▸" context-menu group offering named templates that materialise a new file in the current
  directory (name prompt or sensible default), then hand it to the F4 editor (0086/0099).
- Templates are: (a) a small set of built-ins the user can enable/disable and edit in-place, and
  (b) plugin-contributed templates (0053) so extensions can add their own — no core bloat.
- Safe: never overwrites an existing file (goes through the conflict/confirmation flow, 0045);
  placeholders in templates are optional.
- Cross-host parity; template storage under settings (0030), per-user with an easy reset.

## Implementation Notes
- Model a template as `{ id, label, mimeType?, body }`; the built-in store is just editable JSON in
  settings, and plugins contribute via the existing manifest/registry path.
- Reuse the create-file operation so undo/history (0160) and the operation centre stay consistent.

## Agent Notes
- 2026-11-08 claude: Built-in/user templates done end to end; plugin-contributed templates are NOT.
  Backend: `fm-settings` `FileTemplate` (`id,label,fileName,body,enabled`), `default_file_templates()`
  (text, markdown, to-do, shell, HTML, JSON), `sanitize_file_templates` on load+save (single-segment
  file names only, unique ids, bounds 30 templates / 64 KiB body), DTO + mapping, OpenAPI/Orval
  regenerated, `core.newFromTemplate` registered (no shortcut). Frontend: `features/templates/
  file-templates.ts` (`{date}`/`{name}` placeholders, optional), `NewFromTemplateDialog` (pick
  template, pre-selected file name, validated), creation via the ordinary `createTextFile` operation
  (never overwrites, conflict flow, operation centre) then F4 editor opens; palette + empty-area
  context menu entry (hidden/disabled on read-only locations); Settings editor to enable/edit/add/
  remove/reset (reset uses a frontend copy of the defaults - keep in sync with `fm-settings`).
  Tests: 2 Rust (defaults, sanitising), 6 frontend (placeholders, enabled filter, dialog x3,
  palette routing) + availability.
  **Open:** a "New >" submenu (the context menu only has bespoke Open With/platform submenus, so it
  is a single "New File from Template..." entry opening the picker); not verified visually / in a
  real host; mock action fixture (`fixtures/mock-responses/actions.json`) does not list the new
  action, like the other recent client-flow actions.
- 2026-11-11 claude: Three template sources now merged by backend `GET /api/v1/file-templates`
  (`listFileTemplates`, Tauri `list_file_templates`; `fm_application::file_templates::catalog`):
  (1) app templates from settings (enabled only); (2) user **template folder** — setting
  `templateFolder` (`fm-settings`, edited in Settings → Terminal beside the list); regular,
  non-hidden, UTF-8 files ≤64 KiB directly inside it, sorted, max 30, symlinks skipped; (3) **plugin**
  templates — manifest `contributions.file_templates = true` + sibling `file-templates.json`
  (`fm_plugin_api::parse_file_templates`, validated; invalid document disables the plugin with a
  diagnostic). Entries carry `source` (`app|folder|plugin`) + `sourceName`; ids `folder:<file>` /
  `plugin:<plugin>:<id>`; the picker shows the source as a suffix tag. Picker loads the catalogue
  on open (`dialogs.setNewFromTemplateEntries`), falling back to settings templates. Built-ins
  extended: Meeting note, Python script, TypeScript module, CSV table, Git ignore; "Shell script"
  relabelled "Bash script" (id unchanged). Defaults live in `frontend/src/features/templates/
  defaults/` (`index.json` + one `<id>.tmpl` body per template), bundled by `fm-settings` and
  loaded by the frontend via `import.meta.glob` (single source).
  Folder source reads the host's filesystem (desktop: local; server: the server host).
  Tests: Rust (catalog, folder scan, plugin manifest + discovery, settings sanitise), frontend
  (catalogue entries, source tags). Also fixed two pre-existing test breakages from b9d4f3c9
  (`querySelectorAll` selector; stale `autocapitalize` expectation).
  Review follow-ups: the catalogue takes only templates and folder (no `Settings` clone); filename
  check is `fm_domain::location::validate_name` in both settings and plugin manifests; a failed
  catalogue load shows a toast (`newFromTemplate.loadFailed`) and falls back to app templates.
  Accepted: folder file bodies are readable by any authenticated API client in server mode.
