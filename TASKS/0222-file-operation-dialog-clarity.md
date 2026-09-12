# 0222 File operation dialog clarity

Status: done
Priority: high
Subsystem: frontend
Depends on: 0044, 0045, 0112

## Context

Copy, move, Trash, permanent-delete, and conflict dialogs did not consistently communicate their
default action. Window-focus restoration could move focus from an open modal back to a file pane,
after which Tab switched panes instead of cycling dialog controls. Copy and move confirmations also
used a generic title, encoded paths were difficult to read, conflict controls used inconsistent
labels and layout, and binary F4 failures occupied an editor pane instead of transient feedback.

## Acceptance Criteria

- Routine operation dialogs use the action as their localized title and initially focus that action.
- Tab and Shift+Tab cycle deterministically through every enabled control without escaping the
  active dialog, including after the application window regains focus.
- The focused action uses one solid, high-contrast treatment; destructive actions use the error
  colour, and checkbox focus remains visible without an outer button ring.
- Copy and move confirmations use correct localized singular/plural wording and readable decoded
  display paths without modifying operation URIs.
- The conflict dialog uses the shared confirmation layout and localized Cancel/Rename labels.
- Binary or invalid UTF-8 F4 failures produce a localized toast without leaving an editor pane open.

## Implementation Notes

- Keep operation values and raw location URIs unchanged; decoding is presentation-only.
- Reuse Mithril Materialized `AlertDialog`, but own the file-operation focus cycle because the
  library's document-level trap can conflict with pane-level Tab handling after focus restoration.
- Preserve keyboard access to the conflict dialog's apply-to-all checkbox.

## Agent Notes

- 2026-09-12 Copilot: Completed in PR #43. Added shared operation-dialog focus cycling, guarded
  AppShell window-focus restoration while a modal is active, introduced solid focused-action and
  checkbox-focus styling, aligned the conflict dialog with operation confirmation cards, localized
  all new copy across eight catalogues, and converted binary F4 failures to toasts. Live Chrome
  verification covered Copy and conflict focus loops; affected tests and full repository lint pass.
