# 0126 Embedded terminal drawer

Status: done
Priority: medium
Subsystem: frontend
Depends on: none

## Context

The file manager already supports "Open terminal here", which launches an external terminal in the current folder.

Add a Marta-style embedded terminal drawer that can be toggled from anywhere in the application. The terminal should complement the file manager workflow rather than replace it.

The goal is to reduce context switching by allowing users to execute commands, Git operations, package manager commands, and other CLI tools directly within the file manager while remaining focused on file navigation.

## Acceptance Criteria

- A terminal drawer can be toggled with `Ctrl+\``.
- A terminal drawer can be toggled with `F12` in tauri too.
- The drawer appears docked at the bottom of the main layout.
- The drawer can be hidden and shown without losing the active terminal session.
- A newly created terminal session starts in the currently active directory.
- Switching folders in the file manager does not unexpectedly reset an existing terminal session.
- Terminal sessions belong to locations, not UI panes.
- Opening a terminal for a location that already has an associated terminal session reuses the existing session.
- Multiple panes showing the same location use the same terminal session.
- When multiple panes are visible, toggling the terminal uses the currently active pane's location.
- The terminal is resizable.
- Resizing the terminal drawer resizes the underlying terminal correctly without losing terminal state.
- The terminal supports interactive CLI applications through a PTY backend.
- Terminal input/output is rendered correctly, including ANSI colors.
- The feature works on all supported desktop platforms.
- The existing "Open terminal here" functionality remains unchanged.

## Implementation Notes

- Use xterm.js for terminal rendering.
- Use portable-pty in the Rust backend.
- Create a terminal registry keyed by workspace location.
- Each location owns a single persistent terminal session.
- Opening a terminal for an existing location reuses the existing session.
- Multiple file manager panes showing the same location share the same terminal.
- Terminal sessions remain alive while hidden.
- The architecture must support future remote locations (SSH) without redesign.
- Terminal is hidden/shown via F12 and Ctrl+`.
- New session starts in the active folder.
- Out-of-scope
  - Terminal list / switcher
  - SSH-backed workspace locations
  - Reconnect to existing SSH terminals
- Persist terminal metadata between sessions
- A workspace location is uniquely identified by its backing filesystem location.
- The identifier must be abstract enough to support future local and remote (SSH) locations.
  Examples:
  - local:/projects/foo
  - local:/projects/bar
  - ssh://server1/projects/foo

Relevant areas:

- Main application layout
- Keyboard shortcut handling
- State management
- Desktop/PTY integration layer
- Terminal component abstraction

## Non-Goals

- Terminal tabs
- Split terminal views
- SSH connections
- Terminal session persistence across application restarts
- Dedicated terminal management UI

## Agent Notes

- Feature intentionally targets the "sweet spot" between a simple command line and full IDE-style terminal management.
- Existing external terminal support provides a fallback and reference implementation for determining the working directory.
- The first implementation should support multiple persistent terminal sessions through the location registry.
- Only one terminal drawer is visible at a time.
- A dedicated terminal switcher UI is out of scope for this task.
- 2026-08-11 Codex: Implemented a Tauri-owned `portable-pty` registry keyed by abstract local
  location identifiers, with persistent sessions, replayable output, IPC input/resize commands,
  and xterm.js rendering in a resizable bottom drawer. Ctrl+backtick selects the active pane's
  location; F12 is desktop-only. Browser mode and the existing external-terminal action are
  unchanged. Verified with the fm-desktop tests and focused shortcut tests. The complete frontend
  suite has four pre-existing failures in theme/client expectations (846 tests pass); full
  typecheck also has three pre-existing errors in the HTTP client, conflict-dialog test, and Vite
  config. Windows/Linux were compile-targeted by portable-pty's cross-platform API but not run on
  those operating systems in this macOS environment.
