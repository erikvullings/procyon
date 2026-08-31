# 0001 OS cloud-backed locations

Status: done
Priority: high
Subsystem: backend
Depends on: none

## Context
Add first-class discovery and presentation of cloud-backed filesystem locations already exposed by the operating system, including OneDrive, iCloud Drive, Dropbox, Google Drive, and similar providers. This is intentionally an easy win and must not depend on the remote `ConnectionManager`. These locations continue to use the existing local `FileSystemProvider`.

## Acceptance Criteria
- A platform-facing system-location discovery abstraction exists.
- macOS discovers common cloud-backed locations without hard-coded user-specific paths.
- Windows has an equivalent adapter or documented fallback.
- Discovered cloud locations resolve to the existing `local` provider.
- The frontend shows them in a `CLOUD`/Locations section.
- Opening one behaves like opening a normal local directory.
- Missing/offline providers produce recoverable states.
- No vendor API credentials or `ConnectionProfile` are required.
- Tests cover classification and graceful fallback.

## Implementation Notes
- Introduce `SystemLocationProvider`, `SystemLocation`, and `SystemLocationKind`.
- Add optional advisory `provider_hint` values; never couple file semantics to them.
- Likely areas: `fm-system-locations`, platform adapters, `frontend/src/features/locations`.
- Add `GET /api/v1/system-locations` and equivalent Tauri service path if appropriate.

## Agent Notes
- Inspect current platform adapters, local-provider URI handling, and sidebar/navigation components first.
- 2026-08-09: Added typed platform discovery, local-provider DTO projection, REST and Tauri paths,
  generated frontend bindings, and a recoverable `CLOUD` favourites-menu section. macOS enumerates
  `~/Library/CloudStorage` and the standard iCloud Drive container without embedding a username.
  Windows uses the documented OneDrive/Consumer/Commercial environment-variable fallback and
  omits missing or duplicate directories; this fallback was compile-checked on macOS but was not
  exercised on a Windows host.
- 2026-08-09: Task-specific Rust integration/unit tests and 91 focused frontend tests pass. Full
  macOS adapter tests retain three pre-existing sandbox-sensitive failures (Trash permission,
  mounted-volume enumeration, and Launch Services recommendations). Repository frontend typecheck
  also retains three pre-existing errors in archive request optionality, a conflict-dialog fixture,
  and the Vite config's `.ts` import setting. Rust formatting/clippy pass; repository Biome lint is
  still blocked by pre-existing formatting/lint findings outside this task's files.
- 2026-08-09: Follow-up fixed Tauri double-click handling for discovered roots that macOS reports as
  symlinks. Known cloud locations now navigate inside the active pane through the local provider
  instead of falling through to `core.open` and launching Finder.
- 2026-08-10: macOS discovery now prefers a home-directory symlink whose canonical target is a
  discovered cloud root (for example `~/OneDrive` targeting
  `~/Library/CloudStorage/OneDrive-Personal`). This makes activation of the visible home-directory
  link match the discovered local-provider location and navigate inside the pane.
- 2026-08-10: Axum browser mode now selects the platform adapter for the server host instead of the
  empty fallback adapter. Because browser clients browse the server filesystem, cloud locations
  (including macOS iCloud Drive) are now discoverable and navigable there as well as in Tauri.
- 2026-08-10: Discovered cloud roots are no longer offered as user-addable favourites because they
  already have a permanent Cloud menu entry. The tab-strip button uses `heartPlusIcon` only when
  the current folder can be added, and `heartIcon` otherwise.
