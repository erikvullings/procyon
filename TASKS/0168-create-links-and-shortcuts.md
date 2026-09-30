# 0168 Create symbolic links and Windows shortcuts

Status: done
Priority: low
Subsystem: backend, operations, platform
Depends on: 0035, 0058

## Context

0129 identified creation of links as a remaining Total Commander parity gap. Procyon understands
existing symlinks during listing and copy planning but has no operation for creating a POSIX symbolic
link, Windows symbolic link/junction, or Windows `.lnk` shortcut.

## Acceptance Criteria

- A Create link action presents only link kinds supported for the selected target and destination.
- POSIX symbolic links preserve an explicit relative or absolute target choice and never dereference
  the target while creating the link.
- Windows support distinguishes filesystem links/junctions from shell `.lnk` shortcuts and explains
  privilege or developer-mode requirements before execution.
- Link creation uses an operation-engine job with destination conflict handling, audit history,
  cancellation where meaningful, and HTTP/Tauri parity.
- Remote providers expose the action only when their capabilities define link creation semantics.
- Invalid targets, privilege failures, destination races, cycles, and unsupported providers produce
  typed errors.
- Tests cover file/directory links, relative targets, Unicode, conflicts, privilege denial, remote
  capability gating, and symlink-safe cleanup.

## Implementation Notes

- Split from the candidate table in 0129; update that parent task when this feature is completed.
- Do not treat `.lnk`, NTFS junctions, and symbolic links as interchangeable abstractions.
- Add a VFS capability only where providers can implement it consistently.

## Agent Notes

- 2026-08-28: Promoted from 0129 into a standalone task during the product feature review.
- 2026-09-30 Copilot: Implemented. `core.createLink` (Ctrl+Shift+F5, single selection; context
  menu "organise" group) opens a dialog that loads `POST /api/v1/link-options` / Tauri
  `get_link_options` and offers only the kinds valid for the target/destination pair, each with its
  requirement text (Developer Mode/administrator, local directory, shell-only `.lnk`). Separate
  kinds throughout: `CREATE_SYMLINK`/`CREATE_JUNCTION` provider capabilities in `fm-vfs`
  (`fm-vfs-local`, junctions in `junction.rs`), `CREATE_SHORTCUT` platform capability in
  `fm-platform` (Windows COM `IShellLinkW`). The job is `OperationKind::CreateLink`
  (`fm-application/src/link_operation.rs`): conflict ask/skip/overwrite/rename-new, audit history,
  undo removes only the created link (unavailable once an existing entry was replaced), typed
  `privilegeRequired`/`linkCycle` codes. Relative symlink targets are stored lexically and
  verbatim; junctions and shortcuts are always absolute. Code review fixes: an overwrite whose
  destination resolves to the target itself (case-insensitive name or symlinked parent) fails
  with `linkCycle` instead of deleting the target; Windows directory links follow reparse-point
  targets (OneDrive, junctions) via `resolve_symlink` so they are created as directory links.
  Tests: `fm-vfs-local/tests/create_link.rs`, `fm-application/tests/create_link_operation.rs`,
  the action registry test, and frontend dialog/keydown/operations-controller tests.
  **Not runtime-tested on Windows**: symlink privilege denial, junction and `.lnk` paths are
  covered by cfg(windows) code that was only cross-compiled/clippied (`fm-vfs-local`,
  `fm-platform-windows`); `fm-application` could not be cross-checked locally (aws-lc/ring C build).
