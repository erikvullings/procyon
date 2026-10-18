# 0257 Permissions and ownership (chmod/chown) editor

Status: open
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
