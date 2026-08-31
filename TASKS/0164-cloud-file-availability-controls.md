# 0164 Cloud file availability controls

Status: open
Priority: medium
Subsystem: backend, frontend, platform
Depends on: 0058, 0101, 0134

## Context

OS-mediated OneDrive and iCloud locations can contain online-only placeholders. Lazy thumbnails now
avoid hydrating an entire folder, but Procyon does not show availability state or let users request
the operating system's download, pin, or local-space reclamation actions.

## Acceptance Criteria

- Directory entries expose a normalized availability state such as local, online-only, downloading,
  pinned, or unknown without forcing file hydration.
- Grid and table views show unobtrusive availability/progress indicators with accessible text.
- Supported platforms expose explicit actions for Download now, Keep downloaded, and Free local
  space; unavailable actions are disabled with a reason.
- Downloading is cancellable where the OS API permits it and progress updates do not block directory
  listing or thumbnail virtualization.
- Thumbnail requests continue to hydrate only visible items and never implicitly pin files.
- Platform adapters isolate OneDrive/iCloud-specific APIs; unsupported providers retain normal local
  behavior.
- Tests cover capability mapping, no-hydration metadata reads, action availability, lazy thumbnail
  interaction, errors, and HTTP/Tauri behavior where applicable.

## Implementation Notes

- Extend platform/provider capabilities rather than detecting cloud roots in frontend components.
- Investigate Windows Cloud Files API and macOS File Provider metadata/actions independently; do not
  claim parity for actions an OS does not expose safely.
- Bulk actions must estimate local-space impact and require confirmation before hydrating a large
  selection.

## Agent Notes

- 2026-08-28: Created as the explicit follow-up to lazy cloud thumbnails. The first phase should be
  read-only availability reporting before adding platform mutation actions.
