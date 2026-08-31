# 0002 Mounted network volumes

Status: done
Priority: high
Subsystem: backend
Depends on: 0001

## Context
Add discovery and presentation of network filesystems already mounted by the OS, with SMB/Samba on macOS as the primary use case. Do not implement SMB itself: mounted shares use the existing local provider.

## Acceptance Criteria
- Mounted network volumes are detected by platform adapters.
- Network volumes appear separately from ordinary local/removable volumes.
- Mounted SMB shares open through the local provider.
- Disappearing/unavailable shares leave tabs recoverable rather than crashing/closing.
- Read-only mounts are respected where detectable.
- No native SMB dependency is introduced.
- Tests cover mapping and disappearance/unavailability.

## Implementation Notes
- Extend 0001's `SystemLocationProvider` pipeline.
- Add optional protocol/server/share/read-only metadata.
- Prefer platform metadata over assuming every `/Volumes/*` item is SMB.
- Keep optional OS-level “Mount share…” action out of scope initially.

## Agent Notes
- Reuse 0001's location-discovery model; do not create a second sidebar model for shares.
- 2026-08-09: Extended the shared system-location model, REST/Tauri application mapping, generated
  OpenAPI client, and frontend model with a `network` kind plus optional protocol, server, share,
  and read-only metadata. Network locations remain `local` provider locations and are presented in
  a separate `NETWORK` group.
- 2026-08-09: macOS discovery uses mounted-volume resource metadata (`isLocal`, mount source, and
  read-only state), so it does not infer that every `/Volumes/*` entry is SMB. Windows discovery
  uses the OS drive and WNet APIs for mapped network drives; no SMB implementation or dependency
  was introduced.
- 2026-08-09: A disappeared share now stays as a recoverable tab with the normal directory error
  view. Detected read-only state applies to both the mount root and descendant directories.
- 2026-08-09: Added tests for platform metadata mapping/local-volume exclusion, transport and
  server mapping, frontend grouping/HTTP mapping, disappeared-share recovery, and read-only
  descendants. `cargo test --workspace` and Rust formatting/clippy passed. The full frontend run
  passed 704 tests and retained three unrelated existing failures (theme selector formatting, a
  stale mock action list, and the content-viewer search assertion). Repository-wide Biome lint also
  retains existing CSS/format diagnostics; checks of the files changed for this task pass. The
  frontend type-check retains three unrelated existing errors in archive creation, a conflict-dialog
  fixture, and the Vite configuration.
- 2026-08-09: The Windows adapter is cfg-checked by the host workspace build, but a Windows-target
  cross-check could not run because this machine's listed MSVC target lacks its Rust standard
  library. Runtime validation on Windows remains outstanding.
- 2026-08-09: Follow-up aligned mounted-share activation with cloud locations when the OS reports a
  discovered root as a symlink: double-click navigates inside the active pane through the local
  provider rather than invoking the host's external-open action.
- 2026-08-10: The same macOS home-symlink alias resolution is applied after mounted network volumes
  are added, so a home link targeting an SMB mount is published as the navigable system location
  while retaining its network/protocol/read-only metadata.
- 2026-08-10: Axum browser mode now uses the server host's native platform adapter, restoring the
  same mounted-network discovery and in-pane navigation available in Tauri.
