# 0258 Permissions and ownership (chmod/chown) editor

Status: done
Priority: medium
Subsystem: backend, frontend
Depends on: 0140, 0035, 0058

## Context
The Properties dialog (0140) shows permissions but cannot change them. Users expect to edit mode bits (Unix), owner/group, and Windows attributes (read-only/hidden), optionally recursively (TC Change Attributes, Finder Get Info).

## Acceptance Criteria
- Properties dialog (or separate "Change attributes…" action) edits rwx for user/group/other, setuid/setgid/sticky, owner and group (when permitted), and Windows read-only/hidden/archive attributes; capability-gated per provider/platform via the platform adapter.
- Multi-selection and "apply recursively" (to files only / folders only / both) as a cancellable job through the operation engine; preview of change count before applying.
- Clear errors for EPERM, unsupported providers (SFTP supported where the protocol allows; others hidden/disabled with reason).
- Undo (0160) records previous modes where feasible, else states it is not undoable.
- Symlinks are not followed; tests only in temp roots.

## Implementation Notes
- Extend `fm-vfs` capability flags + local and SFTP providers; Windows via `fm-platform-windows`.
- Related: 0145 (tags/comments in Properties), 0136 (xattrs editor).

## Agent Notes
- Created 2026-10-18 from the feature gap review. No work started.
- 2026-10-24 claude: Implemented end-to-end (Rust + frontend, mock/HTTP/Tauri parity).
  - **`fm-vfs`**: new `AttributeSupport`/`AttributeSnapshot`/`AttributeChange` (+ `mode_mask` so a mixed
    multi-selection changes only touched bits; `resolve_mode`, `is_applied_to`, `inverse_from`) and three
    default-unsupported provider methods `attribute_support`/`read_attributes`/`change_attributes`, gated by the
    existing `SET_PERMISSIONS` capability. Providers refuse symlinks (`UnsupportedCapability`), never follow them.
  - **Providers**: local unix `lchown` + `chmod` (std only, owner first, mode re-read after chown); local Windows
    read-only/hidden/archive via `SetFileAttributesW` (in `fm-vfs-local`, which already owns the `windows-sys`
    dep, rather than `fm-platform-windows`); SFTP via `SETSTAT` (mode + uid/gid, windows attrs unsupported) and
    now advertises `SET_PERMISSIONS`. `fm-ssh` fixture gained a `setstat` handler. FTP/WebDAV/S3/OneDrive/archive
    stay unsupported and report `providerUnsupported`.
  - **Operation engine**: new `OperationKind::ChangeAttributes` (`fm-application/src/attribute_change.rs`):
    plan enumerates without following links (children-before-parent order), scope files/folders/both, recursion,
    per-entry refusals (EPERM etc.) become warnings with a clear message and the rest continues; cancellable.
    Undo: `UndoAction::RestoreAttributes` records previous values and refuses when the entry no longer has the
    values the operation set. `StartOperationRequestDto.attributeChange` added (all literals updated).
  - **Transport**: `POST /api/v1/attributes/inspect` (per-provider support + current state of each entry) and
    `/attributes/preview` (change count, files/folders, skipped symlinks, truncation at 250k); Tauri commands
    `inspect_attributes`/`preview_attribute_change`; roots enforced on the server. OpenAPI + Orval regenerated.
  - **Frontend**: `features/attributes/` (`attribute-form.ts` tri-state rwx/setuid/setgid/sticky, octal field,
    numeric owner/group, Windows attrs; `change-attributes-dialog.ts` with recursive scope + live preview), action
    `client.changeAttributes` (palette, needs selection), "Change attributes..." button in Properties, operation
    centre label, `FileManagerClient.inspectAttributes/previewAttributeChange` in mock/HTTP/Tauri clients,
    `OperationsController.changeAttributes`, i18n in all 8 locales.
  - **Verified**: `cargo test -p fm-application --test change_attributes_operation` 11 tests (chmod+undo, undo
    refused after foreign change, scopes both/files/folders with symlink + outside target untouched, multi-selection,
    selected symlink warning, EPERM per-entry warnings, mode-mask+undo, invalid/unsupported requests, preview counts,
    inspect, archive provider refused); `fm-vfs-local --test attributes` 5; `fm-vfs-sftp --test provider` +2
    (setstat chmod/chown EPERM, windows attrs unsupported); `fm-vfs` lib +4; `fm-server --test files_routes`
    +1; frontend 9 (`attribute-form`) + 8 (`change-attributes-dialog`) + 1 controller + 1 properties + 1 mock
    client; full frontend vitest, `tsc --noEmit`, `pnpm run lint` clean; Windows code type-checked with
    `cargo clippy --target x86_64-pc-windows-msvc` only (not run).
  - **Known gaps**: owner/group are numeric ids (no name resolution, as in 0140); the Windows `SetFileAttributesW`
    path and the Windows test are compiled but untested on Windows; macOS `.app` bundles are treated as a single
    file by the listing, so recursion does not descend into them; ACL editing is out of scope; undo of a change that
    later gets edited by something else is refused rather than merged.
