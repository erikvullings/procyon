# 0096 Mounted volume capacity (total/available disk space)

Status: done
Priority: low
Owner: unassigned
Agent: claude
Area: cross-cutting
Depends on: none

## Context
Follow-up from the pane status bar polish pass (task the Marta-style status bar summary was built
under). The user wants the pane status bar to show a Marta/Finder-style trailing segment like
`616.04 GB (30%) available`, describing the free/total capacity of the volume the active location
lives on.

No such capability exists anywhere in the codebase today (confirmed by exhaustive grep for
`freeSpace|availableSpace|diskFree|free_space|available_bytes|statvfs|disk_usage|volume_capacity`
etc. across both frontend and backend — no hits besides unrelated operation-progress
`total_bytes` fields). Specifically:
- `PlatformAdapter` (`crates/fm-platform/src/adapter.rs`) has a `mounted_volumes()` method but no
  per-location or per-volume capacity query.
- `MountedVolume` (`crates/fm-platform/src/types.rs`) only has `name`/`mount_point` fields, no
  capacity/free-space fields.
- No DTO, OpenAPI schema, Orval-generated client method, or mock-client field exists for this.

## Acceptance Criteria
- `PlatformAdapter` gains a method (e.g. `volume_capacity(&self, path: &Path) -> Result<VolumeCapacity, PlatformError>`)
  returning total and available bytes for the volume containing `path`, with a
  `PlatformCapabilities` flag so unsupported adapters degrade gracefully (existing trait
  convention - see the doc comment on `PlatformAdapter`).
- macOS and Windows adapters (`fm-platform-macos`, `fm-platform-windows`) implement it using the
  native APIs (e.g. `statfs`/`NSFileManager` attributes on macOS, `GetDiskFreeSpaceExW` on
  Windows). Follow the existing sync-native-call-via-`spawn_blocking` convention (spec §28).
- New DTO field(s) surfaced through the existing directory-listing response (or a small dedicated
  endpoint/Tauri command - decide based on how often this should refresh vs. how expensive the
  native call is; a per-directory-listing call is probably the simplest place to hook it in) plus
  the corresponding OpenAPI schema and regenerated Orval client (`pnpm run api:export` then
  `pnpm run api:generate` - do not hand-edit `frontend/openapi/openapi.json` or the generated
  client, per `AGENTS.md`).
- Mock client (`frontend/src/api/client/mock-file-manager-client.ts`) returns a plausible
  synthetic value so the UI is exercisable/testable in mock mode and in component tests without a
  real backend.
- Frontend: pane status bar (`frontend/src/features/panes/pane.ts`) appends a
  `"<available> (<percent>%) available"` segment when the capacity is known, omitting it
  gracefully (not a broken/placeholder string) when unsupported for the current
  provider/platform (e.g. non-local providers, or hosts with no adapter support).
- Tests: adapter-level unit tests per platform (following this repo's platform-adapter test
  conventions), and a frontend pane test asserting the status bar renders the available-space
  segment when the attrs provide it and omits it when absent.

## Agent Notes
- 2026-08-13 claude: Implemented end to end.
  - `fm-platform`: added `VolumeCapacity { total_bytes, available_bytes }`, the
    `PlatformCapabilities::VOLUME_CAPACITY` flag, and
    `PlatformAdapter::volume_capacity(&self, path: &Path)` with an
    `Unsupported` default (mirrors the existing trait convention).
  - `fm-platform-macos`: implemented via `NSFileManager
    -attributesOfFileSystemForPath:error:` (`NSFileSystemSize`/
    `NSFileSystemFreeSize`), synchronous like every other adapter method here
    (native calls are already run through `spawn_blocking` by the caller, per
    spec §28).
  - `fm-platform-windows`: implemented via `GetDiskFreeSpaceExW`
    (`lpfreebytesavailabletocaller`/`lptotalnumberofbytes`), same pattern.
  - `fm-transport-dto`: added `VolumeCapacityDto` and an optional
    `volume_capacity` field on `DirectorySnapshotDto` (the domain
    `DirectorySnapshot` intentionally stays platform-agnostic; `From<DirectorySnapshot>`
    always produces `None` there).
  - `fm-application`: `FileManagerService::{list_directory,refresh_directory,
    navigate_pane}` now return `DirectorySnapshotDto` directly (previously the
    domain `DirectorySnapshot`) so they can attach the backing volume's
    capacity in one place (`enrich_snapshot`/`volume_capacity`), gated on the
    adapter's `VOLUME_CAPACITY` capability and `Location::to_native_path`
    (so non-local providers and unsupported platforms degrade to `None`
    rather than erroring the whole listing). Updated the two callers
    (`apps/fm-server/src/routes/directory.rs`,
    `apps/fm-desktop/src-tauri/src/commands.rs`) accordingly.
  - OpenAPI/Orval: regenerated via `pnpm run api:export` +
    `pnpm run api:generate` (not hand-edited).
  - Mock client (`mock-file-manager-client.ts`): returns a fixed plausible
    capacity for `listDirectory`/`navigatePane`, omitted for `search://`
    results (mirrors the real non-local-provider gap).
  - Frontend: `models/snapshot.ts`, `features/navigation/navigation.ts`,
    `features/workspace/{pane-content-builder,workspace-layout}.ts` thread
    `volumeCapacity` through; `features/panes/pane.ts` appends a
    `"<available> (<percent>%) available"` `.fm-pane-volume-capacity` status
    bar segment when known, renders nothing otherwise.
  - Tests: macOS/Windows adapter unit tests (boot volume/temp dir capacity,
    not-found path); `fm-transport-dto` DTO round-trip/camelCase/schema
    tests; `fm-application` service tests (`list_directory` attaches
    capacity when the adapter supports it, omits it when the capability is
    absent, and omits it for a non-local `search://` location); a
    `pane.test.ts` pair asserting the segment renders with the correct
    label and is absent when `volumeCapacity` is undefined.
  - Verified: `cargo test -p fm-platform -p fm-platform-macos
    -p fm-transport-dto -p fm-application` (all green, incl. 3 new
    volume-capacity service tests and 2 new macOS adapter tests),
    `cargo test -p fm-server` (all green), `cargo clippy` clean on every
    touched crate plus `fm-desktop`, `cargo fmt --check` clean; frontend
    `pnpm run typecheck` clean, `npx vitest run` (916/916 passed, incl. 2 new
    pane tests), `npx biome check` clean on every touched file.
  - Known gap: the Windows adapter implementation could not be compiled or
    executed on this (macOS) development machine — I reviewed the
    `windows-sys` `GetDiskFreeSpaceExW` signature directly against the
    `windows-sys` 0.61 source to match it exactly, but it has only been
    verified by inspection, not by a local build. CI runs
    `cargo test --workspace` on `windows-latest`, which will exercise it.
