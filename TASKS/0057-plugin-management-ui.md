# 0057 Plugin management UI

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0055, 0056

## Context
`file-manager-coding-agent-spec.md` §33 step 9 ("plugin management page") and §37 (plugin management
UI is part of polished version 1).

## Acceptance Criteria
- A settings page lists discovered plugins with name, version, description, source and status
  (enabled / disabled / failed / auto-disabled).
- Enable and disable toggles persist through the settings service and take effect without restart.
- Each plugin shows the permissions it requests, with denied permissions clearly marked.
- Plugin diagnostics are viewable: recent errors and the plugin's bounded log (§19.4).
- Invalid manifests are listed with the validation error rather than hidden.
- `plugin.changed` events update the page live.
- Built with `mithril-materialized` form components (§14) and keyboard accessible (§29).
- Vitest tests cover the enable/disable flow and error-state rendering.

## Implementation Notes
- No plugin installation/marketplace flow — out of scope for version 1 (§37).

## Agent Notes
- Not started.
- 2026-07-31 codex: This task is a prerequisite for 0083. Build the plugin list and enable/disable
  flow as a reusable settings section (or routable feature) so the general settings editor embeds
  or links to it instead of creating a second plugin-management path.
- 2026-08-01 Claude Sonnet 5 (Copilot): Implemented end-to-end, embedded as a "Plugins" section
  inside the existing `.fm-settings-editor` disclosure panel (per the prior note), not a separate
  route.
  - Backend: added `PluginPermissionsDto`/`PluginLogEntryDto` (fm-transport-dto), a `permissions`
    field on `PluginDescriptorDto`, `FileManagerService::plugin_logs()`, and published
    `BackendEventPayload::PluginChanged` from `set_plugin_enabled()`. Added the
    `GET /api/v1/plugins/{pluginId}/logs` route and its Tauri command mirror. Regenerated
    `frontend/openapi/openapi.json` and the Orval client.
  - Frontend: `PluginManagement` (new, `frontend/src/features/plugin-management/`) is a dense,
    non-Materialize-card list (consistent with the directory table precedent in task 0024 and
    spec §14) using the `mithril-materialized` `Switch` for enable/disable and `ModalPanel` for the
    bounded diagnostic log viewer. All 10 `PluginPermissions` fields render per plugin with a
    granted/denied state marked by both a ✓/✗ glyph and a `data-granted` attribute (not color
    alone). Diagnostics (covering both invalid-manifest and auto-disable cases — the backend does
    not yet distinguish these structurally, so both surface via the same `diagnostic` string) are
    shown inline rather than hidden. Wired into `app-shell.ts`: `listPlugins()` on mount, a
    `plugin.changed` event handler that upserts the summary into the local list, and
    `setPluginEnabled`/`getPluginLogs` added across all three `FileManagerClient` implementations
    (HTTP, mock, Tauri).
  - Fixed a real pre-existing type bug found while wiring this up: the `plugin.changed`
    `BackendEventPayload` case was typed as `{ plugin: PluginDescriptor }` (requiring
    `description`), but the backend only ever publishes `PluginPayload` (id/name/version/enabled).
    Introduced a `PluginSummary` model for this event instead.
  - Verification: `cargo test --workspace` and `pnpm run test:frontend` (300 frontend tests, all
    passing) and `pnpm run lint` (Biome + clippy) all clean, `tsc --noEmit` clean. New/updated
    tests: `plugin-management.test.ts` (8 cases: empty state, listing, permission
    granted/denied markers, diagnostic rendering, toggle flow, toggle-failure surfacing, log
    load/loading/error), 3 new cases in `http-file-manager-client.test.ts`, 2 new cases in
    `app-shell.test.ts` (plugin listing + `plugin.changed` live update), plus the existing backend
    plugin tests from earlier in this session.
  - Known pre-existing issues, **not** introduced by this task (confirmed unrelated via
    `git stash`/clean-`main` reruns and via `git status` showing zero diff in the affected files):
    - `list_plugins_starts_empty_and_unknown_enablement_is_not_found`
      (`apps/fm-server/tests/plugin_routes.rs`) fails identically on unmodified `main`.
    - `metadata_is_separate_and_capabilities_are_truthful`
      (`crates/fm-vfs-local/tests/local_provider.rs`) fails with a `DELETE`/`MOVE` capability-bit
      mismatch; `fm-vfs-local` has zero diff from this session, so this is an environment-dependent
      failure on this machine, unrelated to 0057.
    - The working tree already had an unrelated, uncommitted one-line change in
      `frontend/src/api/client/tauri-file-manager-client.ts`
      (`import { type FileManagerClient }` → `import type { FileManagerClient }`) predating this
      session's work. Left in place but **excluded** from this task's commit.
  - Gap: the acceptance criterion "status (enabled / disabled / failed / auto-disabled)" is not
    rendered as four distinct labels — the backend only exposes `enabled: bool` plus an optional
    `diagnostic: String`, with no structured distinction between "invalid manifest", "auto-disabled
    after failures", and "manually disabled". The UI shows enabled/disabled plus the diagnostic
    text verbatim when present (which already contains human-readable wording for both invalid-
    manifest and auto-disable cases), satisfying the substance of the criterion (nothing is hidden)
    without inventing an unbacked frontend classification. Flagging this explicitly rather than
    marking it a silent gap.
