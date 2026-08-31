# 0138 OS-level "Mount share…" action

Status: open
Priority: low
Owner: unassigned
Agent: unassigned
Area: platform
Depends on: 0102

## Context

0102 (mounted network volumes) deliberately kept this out of scope: "Keep optional OS-level 'Mount
share…' action out of scope initially" — fm currently only *presents* shares the OS has already
mounted, it can't initiate a new mount itself. This task is the deferred follow-up: an in-app action
that prompts for a server address/share (and credentials, handled via the OS keychain/credential
manager, never stored by fm) and asks the OS to mount it, after which it appears through 0102's
existing discovery/presentation path.

## Acceptance Criteria
- A new action (command palette + context menu entry, e.g. on a "Network" sidebar group) that
  prompts for a share address (`smb://server/share` or platform-equivalent) and triggers the OS's
  native mount flow — macOS `NetFSMountURLSync` or equivalent, Windows `WNetAddConnection2`.
- Credentials are handed to the OS's native prompt/keychain, never captured or stored by fm directly
  (consistent with the "prohibited: entering passwords" boundary already respected elsewhere in the
  app).
- On success, the newly mounted share appears via 0102's existing discovery without a manual
  refresh.
- Capability-gated: report `false` on Linux/browser mode rather than a partial implementation.
- Tests: platform adapter unit tests for the mount-request plumbing where feasible without a real
  network share; manual verification recorded.

## Implementation Notes
- Low priority — 0102 already covers the common case (shares mounted via Finder/Explorer show up in
  fm). Only worth doing if users hit real friction switching to the OS's own mount UI.

## Agent Notes
- (none yet)
