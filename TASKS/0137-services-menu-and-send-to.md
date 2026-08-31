# 0137 Services menu (macOS) / "Send to" (Windows) integration

Status: in_progress
Priority: low
Owner: unassigned
Agent: unassigned
Area: platform
Depends on: 0058, 0059, 0060

## Context

macOS's Services menu (right-click → Services) and Windows' "Send to" context-menu submenu let
other installed apps register themselves as targets for the selected file(s) — e.g. "Send to Mail
recipient", a Automator/Shortcuts workflow, a compression utility. fm's context menu (0052)
currently only shows fm's own actions/plugins; neither OS integration point is wired up.

## Acceptance Criteria
- macOS: selected entries are exposed to the Services menu (implement `NSServicesMenuRequestor` or
  the modern equivalent) so OS-registered services appear in fm's right-click menu under a
  "Services" submenu, matching Finder's behaviour.
- Windows: fm's context menu includes a "Send to" submenu populated from the user's
  `shell:sendto` folder, matching Explorer's behaviour.
- Both integrate into 0052's existing context-menu construction rather than a parallel menu system.
- Capability-gated: report `false`/omit the submenu on Linux and in browser mode rather than
  half-implementing an equivalent.
- Tests: platform adapter unit tests for submenu population where feasible; manual verification
  recorded for both platforms.

## Implementation Notes
- Lower priority than 0133 (menu bar content) and 0136 (Finder tags/xattrs) — this is a nice-to-have
  interop feature, not a workflow-blocking gap. Pick up after those if capacity allows.

## Agent Notes
- 2026-08-28 copilot: Added a capability-gated native submenu trigger to task 0052's existing
  selection context menu. The Tauri command validates local selections and schedules native menu
  work on the UI thread; browser, mock, Linux, empty, and non-local selections omit the submenu.
  macOS uses an `NSServicesMenuRequestor`-compatible `NSResponder` to expose selected file URLs and
  opens AppKit's OS-populated Services menu. Windows resolves `FOLDERID_SendTo`, deterministically
  enumerates visible destinations, opens a Win32 popup at the pointer, copies to direct folder
  destinations, and invokes other registered destinations with every selected path.
- Automated verification covers the shared context-menu composition and IPC dispatch, Tauri
  selection validation, desktop-only capability reporting, macOS pasteboard type support, and
  Windows Send To discovery/filtering/sorting/argument quoting. macOS and Windows manual UI
  verification remain outstanding (implementation host: macOS 26.6.2, build 25G83), so this task
  intentionally remains `in_progress`. Six added tests execute on macOS; three additional Windows
  adapter tests cross-compile under `cargo clippy --target x86_64-pc-windows-msvc --all-targets`.
  `pnpm test`, `pnpm lint`, and the frontend typecheck pass. Windows drop-only handlers may still
  differ from Explorer because non-directory destinations are invoked through `ShellExecuteW`
  rather than the shell's `IDataObject`/`IDropTarget` pipeline.
- 2026-08-28 copilot: A local Tauri smoke test opened the native Services menu but AppKit reported
  "No Services Apply." The macOS Services registry showed that installed file services advertise
  the legacy `NSFilenamesPboardType`, while the requestor accepted only `public.file-url`. The
  requestor now negotiates both types and publishes the selected paths as the legacy filename-list
  property list when requested. A successful invocation of an installed service still needs a
  manual retest before this task can be marked complete.
