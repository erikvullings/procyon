# 0083 Settings editor UI

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0050, 0057

## Context
Task 0030 provides versioned settings persistence and equivalent HTTP/Tauri APIs, but users cannot
inspect or change those settings in the application. Tasks 0050 and 0057 own the keybinding and
plugin-management behavior that must be represented in a coherent settings experience rather than
added later as disconnected screens.

## Acceptance Criteria
- A keyboard-accessible settings surface can be opened from the application shell and closed
  without losing the current workspace state.
- Sections cover appearance (theme, font size, row height, date and size formats), file behavior
  (hidden files and permanent-delete confirmation), operations (conflict policy and concurrency),
  new-workspace defaults (pane layout, columns and start locations), terminal command, keybindings,
  and plugins.
- Initial values come from `FileManagerClient.getSettings`; saving sends one complete settings
  document through `updateSettings` and applies visible changes without an application restart.
- Editing can be cancelled without persisting changes. Save failures keep the user's draft visible
  and surface an actionable error instead of silently reverting it.
- Keybinding controls consume 0050's conflict detection and platform/browser availability model.
- The plugin section embeds or links to 0057's management UI rather than implementing a second
  enable/disable path.
- Forms use `mithril-materialized`, have associated labels and validation messages, and are usable
  with keyboard navigation.
- Vitest tests cover loading, editing and saving, cancellation, validation/error handling, live
  appearance updates, and the keybinding/plugin integration boundaries.

## Implementation Notes
- Keep draft/form state in a feature model or service; do not move application logic into Mithril
  components.
- Treat default pane layout, columns and start locations as defaults for newly created workspace
  content. Never overwrite the live layout, tabs or per-tab view configuration of an open workspace.
- Do not store secrets in settings, and do not hand-edit generated OpenAPI or Orval files.
- Implement after 0050 and 0057 are done so the editor integrates their final models. Task 0030 is
  already complete and supplies the persistence contract.

## Agent Notes
- 2026-07-31 codex: Created after 0030 delivered persistence without a user-facing editor. Best
  implementation point is immediately after 0050 and 0057; those tasks should leave reusable
  keybinding and plugin-management feature boundaries for this screen.
- 2026-08-03 Claude Sonnet 5 (Copilot): Implemented `features/settings/settings-model.ts` (draft
  clone/validation/list-field parsing/keybinding-override helpers) and
  `features/settings/settings-editor.ts` (all sections: appearance incl. `ThemeSwitcher`, file
  behavior, operations, new-workspace defaults, terminal command, keybindings with 0050's conflict
  detection and browser/platform availability, and 0057's `PluginManagement` embedded directly).
  Wired into `app/app-shell.ts`: the settings disclosure now renders `SettingsEditor` once
  `getSettings()` resolves, with `onPreview` applying appearance live, `onSave` persisting the full
  document via `updateSettings`, and `onCancel` reverting. 20 new Vitest cases across
  settings-model.test.ts (12) and settings-editor.test.ts (8) cover loading, live preview, cancel,
  save + save-error, validation, keybinding conflicts, and the plugin-management boundary;
  `app-shell.test.ts` covers open/close, theme switching, and plugin listing/events through the new
  editor. Full frontend suite (369 tests), `tsc --noEmit`, and `biome check` all clean.
- 2026-08-30: Fixed the settings panel opening with only its title when WebKit opened the native
  `<details>` disclosure without going through the toolbar callback. The DOM `open` state and the
  separate `settingsDialogOpen` flag diverged, so `AppShell` deliberately omitted
  `SettingsEditor`. The disclosure's native `toggle` event now synchronizes application state in
  both directions. An app-shell regression test opens the disclosure through that native path and
  asserts the editor body, theme controls, and action footer are all present.
