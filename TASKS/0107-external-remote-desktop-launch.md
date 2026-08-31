# 0007 External remote desktop launch

Status: open
Priority: medium
Subsystem: backend
Depends on: 0003

## Context
Allow a saved connection to launch an external RDP or VNC client. Do not embed remote-desktop rendering in this task.

## Acceptance Criteria
- Connection config can optionally define RDP/VNC settings.
- `Open Remote Desktop` exists in the action registry.
- macOS and Windows launch an available/configured client via platform adapters.
- No hard dependency on a single third-party client is introduced.
- Launch failures are structured and user-visible.
- Credentials are handled safely and not casually placed in command lines.
- Automated tests mock platform launch behavior.

## Implementation Notes
- Suggested crate: `fm-remote-desktop`.
- Keep launch/session logic separate from VFS.
- SSH tunneling, embedded RDP and embedded VNC are separate future features.

## Agent Notes
- Inspect existing platform “open with/default application” infrastructure before adding process-launch plumbing.
