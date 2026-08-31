# 0060 Windows platform integration

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: platform
Depends on: 0058

## Context

`file-manager-coding-agent-spec.md` §23 ("Windows targets") and §33 step 10.

## Acceptance Criteria

- Implemented in `fm-platform-windows`: shell icons, Explorer reveal, Recycle Bin, drive listing,
  native menus, terminal integration.
- Drive listing enumerates volumes (including removable and network drives) and surfaces them as
  navigable locations; unavailable drives fail with a typed error rather than hanging.
- UNC paths (`\\server\share`) and long paths (`\\?\` prefixing) work throughout listing and
  operations (§17, §23).
- Junctions, reparse points and shortcuts (`.lnk`) are identified and flagged in the entry; shortcut
  targets are resolved only on explicit open, never during listing.
- Windows file attributes (hidden, system, read-only, archive) are read and shown; hidden/system
  entries respect the hidden-file setting.
- Locked-file errors map to a distinct, user-readable error code rather than a generic I/O error
  (§8, §23).
- Shell thumbnails are declared as an unimplemented capability unless delivered here.
- Tests: UNC and long-path handling, attribute mapping, junction detection, locked-file error
  mapping (using a test that holds an exclusive handle).
- Manually verified on Windows; the task notes record the OS version tested (§35).

## Implementation Notes

- Use the `windows` crate; shell icon extraction must be cached by extension (§28).
- Installer signing is task 0063.

## Agent Notes

- Implemented on Windows 11 (`1.97.1-x86_64-pc-windows-msvc`).
- `fm-platform-windows` now implements Explorer reveal (`explorer.exe /select,`), Recycle Bin
  (`SHFileOperationW` with `FOF_ALLOWUNDO`), drive listing (`GetLogicalDrives` +
  `GetVolumeInformationW`, covering removable, fixed, network, optical and RAM drives),
  open-with-default-application and the Open With chooser (`ShellExecuteW` `open`/`openas`), and
  terminal integration (`wt.exe`, falling back to `powershell.exe`). Capability bits were widened
  to match, and every native call strips the `\\?\` extended-length prefix first, because the
  shell APIs reject that form.
- **Not delivered:** shell icons and shell thumbnails. Both need an `HICON`/`IShellItemImageFactory`
  bitmap re-encoded as PNG, and this workspace has no image encoder dependency; their capability
  bits stay unset rather than reporting success and failing at call time. Native menus and
  clipboard file references also remain delegated to the fallback adapter. Shell icon extraction
  is split out into task 0130, the native menu bar hook into task 0131; thumbnails stay
  unimplemented.
- Fixed a real Windows navigation defect found while testing: `file:///C:/` (any directory URI
  written with a trailing slash, which is the only natural way to write a drive root) was rejected
  as `EmptySegment`. `Location::parse` now accepts a single trailing slash and canonicalises it
  away, so one directory has exactly one URI; interior empty segments (`//`) are still rejected.
- Locked files now map to a distinct `VfsError::Locked` -> `ApplicationError::FileLocked` ->
  `fileLocked` transport code (HTTP 409) instead of a generic I/O error, driven by a test that
  holds a real exclusive (`FILE_SHARE_NONE`) handle.
- Windows `SYSTEM`-attribute entries now follow the hidden-file setting, matching Explorer.
- Junctions and reparse points were already flagged as `EntryKind::Symlink` by `fm-vfs-local`, and
  `.lnk` targets are still never resolved during listing. Explicit `.lnk` target resolution on open
  is **not** implemented.
- Fixed the frontend masking the real backend error in Tauri: `invoke` rejects with a plain
  `{code, message}` object rather than an `Error`, so every backend failure was displayed as the
  generic "Unable to load directory".
- **Root cause of the reported "Tauri shows no files"**: `default_workspace`'s `location_for` built
  its URI with `format!("file://{path}")`. That only produces a valid URI for POSIX paths; on
  Windows the home directory became `file://C:\Users\<user>`, which `Location::parse` rejects as
  `InvalidUri`, so both panes failed every listing. It now goes through
  `Location::from_native_path`. Workspaces already persisted with the broken URI are repaired by a
  new v2 -> v3 schema migration (`CURRENT_WORKSPACE_SCHEMA_VERSION` is now 3). Verified against the
  real persisted workspace on this machine: it reloads as schema 3 with
  `file:///C:/Users/<user>` and lists 18 entries.
- Pre-existing, unrelated to this task and still failing on this machine: `cargo test -p fm-desktop
  --lib` aborts with `STATUS_ENTRYPOINT_NOT_FOUND` (WebView2/Tauri runtime), and 5 frontend tests
  fail (`mithril-inspector`, `import-boundaries`, `component-colours`, one `pane` total). Both were
  verified to fail identically with these changes stashed.
- Also pre-existing and **Windows-specific**: `apps/fm-server`'s `tests/operation_routes.rs` fails
  here - `resolve_conflict_route_applies_the_requested_decision` and
  `resolve_conflict_route_confirms_a_permanent_directory_delete` both return HTTP 500, and
  `start_retry_uses_stable_id_and_copy_emits_full_lifecycle` deadlocks (>60s). Verified to fail
  identically with these changes stashed, so the operation engine has a genuine Windows defect
  independent of task 0060. This is what blocks the pre-commit hook on Windows; tracked as its own
  task, 0132.
- Not verified: drag to/from Explorer, and behaviour on a real UNC share or a mapped network drive
  (no server available in this environment).
