# 0174 macOS Quick Look action

Status: done
Priority: medium
Subsystem: platform, frontend
Depends on: 0059, 0088
Owner: copilot
Agent: copilot

## Context

macOS Quick Look provides high-quality previews for many document, image, media, and creative
formats through system and installed preview generators. Procyon currently offers `core.open`,
`core.openWith`, and its own cross-platform F3 viewer, but the macOS platform adapter explicitly
leaves Quick Look unimplemented.

Add Quick Look as an optional macOS action, not as a replacement for F3. This gives local macOS
users a system-native fallback for unsupported or over-budget formats while preserving Procyon's
cross-platform viewer behavior and the native Office-preview work in 0171–0173.

## Acceptance Criteria

- A capability-gated `core.quickLook` action is available for a single local file on macOS and is
  unavailable in browser/server mode and on unsupported platforms.
- The action appears in the selection context menu and command palette with localized English and
  Dutch copy. Cmd/Ctrl+Y and Shift+F3 invoke it without conflicting with Procyon's existing Space
  select-and-advance, Alt+Space metadata, F3 View, or macOS system shortcuts.
- Invoking the action presents the selected file through Apple's public Quick Look API,
  `QLPreviewPanel` or `QLPreviewView`. Do not depend on the undocumented/debugging-oriented
  `qlmanage` command-line interface.
- Quick Look remains a distinct action: F3 continues to open Procyon's own viewer and Enter continues
  to use the default application.
- Unsupported and over-budget F3 states on macOS offer a visible “View with Quick Look” action
  alongside the existing external-open action when `core.quickLook` is available.
- Closing or replacing the preview releases native panel/view state. Repeated invocation updates or
  focuses the existing preview rather than leaking windows or stale file references.
- Paths are passed as native URL/path objects without shell or AppleScript interpolation. Missing,
  inaccessible, deleted, or unsupported files produce a user-readable result rather than a silent
  failure.
- Tests cover capability gating, action registration and parameter mapping, context-menu/command-
  palette exposure, fallback-button visibility, awkward Unicode paths, panel lifecycle logic, and
  unsupported-platform behavior. Native presentation is manually verified on a supported macOS
  version and recorded in Agent Notes.

## Implementation Notes

- Extend `PlatformCapabilities` and `PlatformAdapter` in `fm-platform`, then implement the capability
  only in `fm-platform-macos`. Keep `fm-application` dispatch provider-neutral and adapters thin.
- Prefer `QuickLookUI.framework` integration through maintained Objective-C bindings. If the current
  `objc2` ecosystem lacks bindings for the required API, isolate minimal FFI in
  `fm-platform-macos`, following its existing `unsafe_code` boundary and main-thread conventions.
- `QLPreviewPanel` participates in the macOS responder chain and window lifecycle. Verify its
  interaction with Tauri's `NSWindow`, focus restoration, app activation, and repeated previews
  before choosing it over embedding `QLPreviewView`.
- Initial scope is local native paths only. Remote SFTP/FTP/WebDAV/S3 and archive-nested entries must
  report the action unavailable; securely materializing bounded temporary files is a separate
  follow-up, not a hidden download inside this task.
- Quick Look quality and supported formats depend on macOS and installed generators. Do not advertise
  format guarantees or use it to remove cross-platform preview renderers.
- Direct Preview.app launching requires no new action: users can already use Enter when Preview is
  the default or Cmd+Enter / `core.openWith` to choose it.

## Agent Notes

- 2026-08-29: Created from the macOS preview discussion. Product direction is an explicit Quick Look
  action plus an F3 fallback affordance, not replacing F3 or adding a Preview.app-specific command.
  Public Quick Look APIs are required; `qlmanage` is deliberately excluded.
- 2026-08-29: Implemented `core.quickLook` through the MIT/Apache-2.0 `quicklook` crate and Apple's
  public `QLPreviewPanel`. The action is capability-gated, has no shortcut, accepts only one local
  `file://` file, and is masked from browser/server and mock runtimes even when those hosts run on
  macOS.
- 2026-08-29: Tauri routes Quick Look invocation onto the macOS main thread. The macOS adapter owns
  one shared panel, replaces its current item on repeated invocation, focuses the existing panel,
  and observes panel close notifications to release retained preview-item URLs. Invalid, missing,
  directory, remote, and off-main-thread requests return explicit application/platform errors.
- 2026-08-29: Unsupported binary, over-budget structured-data, and external-video F3 states now stay
  open and offer localized external-open controls plus “View with Quick Look” when eligible.
  Context-menu, command-palette, mock fixture, and English/Dutch localization wiring were updated.
- 2026-08-29: Native adapter presentation was smoke-tested on macOS 26.6.2 by running the adapter on
  the process main thread against a real local file and keeping the AppKit run loop active. Full
  repository tests and lint pass. Review identified and prompted the browser/server capability mask.
- 2026-08-29: Added Cmd/Ctrl+Y (matching Finder's alternate Quick Look shortcut) and Shift+F3 as
  defaults. Alt+Space remains dedicated to metadata and Space remains select-and-advance.
