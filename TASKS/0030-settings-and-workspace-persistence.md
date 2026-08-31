# 0030 Settings service

Status: done
Priority: medium
Owner: unassigned
Agent: Codex
Area: backend
Depends on: 0019, 0026

## Context
`file-manager-coding-agent-spec.md` §26 (settings, versioned with migrations) and §16 milestone 1.
Settings are persisted by the backend, not in frontend local storage.

**Scope note:** §5.3 was fleshed out in more detail after this task was written. Full workspace
persistence — including which workspace was last active, its panes, tabs, layout and per-tab view
configuration — is now owned end-to-end by tasks 0078–0082 (`WorkspaceRepository`/`WorkspaceService`
and its own `schemaVersion`/migration chain), not by this task. This task keeps only the
application-wide settings below; it no longer restores workspace content itself.

## Acceptance Criteria
- `fm-settings` persists versioned settings to a platform-appropriate config directory
  (`directories`/`dirs` crate), with atomic writes (temp file + rename).
- Settings schema covers the §26 list, at least: theme, font size, row height, date format, size
  format, hidden-file visibility, confirm permanent delete, default conflict policy, operation
  concurrency, default pane layout, default columns, keybindings, enabled plugins, plugin settings,
  terminal command, default start locations. "Default pane layout"/"default columns" here means the
  application-default values a *new* workspace/tab initializes from (§5.3.14's inheritance chain),
  not the live layout/columns of an open workspace — those are workspace content (0078–0082).
- A `schemaVersion` field plus a migration chain; loading an older version migrates rather than
  discarding (§26). A test migrates a v1 fixture to the current version.
- Corrupt or unreadable settings fall back to defaults, back up the bad file, and surface a
  notification — never a crash and never silent data loss.
- `GET /api/v1/settings` and `PUT /api/v1/settings` (`getSettings`, `updateSettings`) with OpenAPI
  schemas; the Tauri host exposes equivalent commands.
- Frontend reads settings at bootstrap and applies theme, row height, formats.

## Implementation Notes
- Do not persist pane layout, open tabs, active pane/tab, split ratio or per-tab sort/columns here —
  those are workspace content owned by 0078–0082 (§5.3.2's ownership table draws this line
  explicitly). This task only owns global defaults that a new workspace initializes from.
- Do not store secrets in settings.

## Agent Notes
- 2026-07-31: Added the versioned `fm-settings` JSON store with platform config-directory
  resolution, atomic replacement, a v1-to-v2 migration, and corrupt-file backup plus global warning
  notification.
- 2026-07-31: Added shared settings DTOs, generated OpenAPI/TypeScript bindings, Axum `GET`/`PUT`
  endpoints, equivalent Tauri commands, and frontend bootstrap application of theme, font size, row
  height, date format, and size format.
- 2026-07-31: Verified six task-specific behaviour tests (three persistence, one recovery
  notification, one REST round trip, and one frontend bootstrap), frontend typechecking,
  `pnpm run lint`, and the complete `pnpm test` suite. Runtime platform-path behaviour was exercised
  on macOS; Windows and Linux were compile/CI-contract tested but not run interactively.
- 2026-08-30: Added `core.extension` to the authoritative and mock default-column lists so new
  settings show Ext by default. Existing saved column choices are preserved.
