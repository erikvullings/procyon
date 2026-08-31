# 0127 External terminal application choice

Status: open
Priority: low
Subsystem: frontend
Depends on: none

## Context
0061 implemented `core.openTerminal`, which launches a single, platform-default external terminal
(`open -a Terminal <path>` on macOS, `PlatformAdapter::open_terminal(path, command_override)`) at
the current directory, with the app overridable only via the global `terminal_command` setting.

Add a context-menu action that lets the user open a *specific* external terminal application
(e.g. ghostty, Warp, iTerm) for the current directory, picked per-invocation rather than only via a
single global setting. This is independent of remote/SSH work (0105) — it applies to any local
location a pane can navigate to.

## Acceptance Criteria
- A context menu entry (e.g. "Open Terminal With...") lists installed/configured external terminal
  applications and launches the chosen one at the current directory.
- Reuses `PlatformAdapter::open_terminal`'s existing safe argument-passing (no shell string
  interpolation of paths) — a new per-app command is passed the same way `terminal_command` is
  today, not built by string concatenation.
- The existing single-default `core.openTerminal` action (F-key/shortcut path) is unchanged.
- The list of available terminal apps is configurable (a settings list of name + launch command),
  with sensible per-platform defaults (e.g. Terminal/iTerm/ghostty/Warp on macOS).
- Capability/context gated the same way as `core.openTerminal` — hidden/unavailable in
  browser-server mode.
- A configured app that isn't actually installed produces a user-readable error, not a silent
  no-op.
- Tests cover argument construction for awkward paths and capability gating; actual launching is
  verified manually per platform and recorded in the task notes (same standard as 0061).

## Implementation Notes
- Extend `fm-settings`/`fm-transport-dto` with a list-of-terminal-apps setting alongside the
  existing single `terminal_command` (`crates/fm-application/src/service.rs`,
  `crates/fm-transport-dto`), rather than replacing it — keep the single-default action's setting
  as is.
- `PlatformAdapter::open_terminal` (`crates/fm-platform/src/adapter.rs`) already takes a
  `command_override: Option<&str>`; the new action can call it once per chosen app without adding a
  new adapter method, unless per-app argument templates turn out to need more structure than a bare
  command string.
- Frontend: new action id (e.g. `core.openTerminalWith`) registered in
  `frontend/src/features/commands/availability.ts` next to `core.openTerminal`
  (`crates/fm-application/src/action.rs`'s `core_actions()`), surfaced in
  `frontend/src/features/commands/context-menu.ts`.

## Agent Notes
- Split out of 0105 on 2026-08-12: the user asked for this as a "second, easier feature" alongside
  SSH remote terminal support, but it's unrelated to SSH/remote connections and was easier to scope
  as its own task.
