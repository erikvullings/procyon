# 0312 New-file-from-template

Status: open
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
- (none yet)
