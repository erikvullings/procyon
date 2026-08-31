# 0168 Create symbolic links and Windows shortcuts

Status: open
Priority: low
Subsystem: backend, operations, platform
Depends on: 0035, 0058

## Context

0129 identified creation of links as a remaining Total Commander parity gap. Procyon understands
existing symlinks during listing and copy planning but has no operation for creating a POSIX symbolic
link, Windows symbolic link/junction, or Windows `.lnk` shortcut.

## Acceptance Criteria

- A Create link action presents only link kinds supported for the selected target and destination.
- POSIX symbolic links preserve an explicit relative or absolute target choice and never dereference
  the target while creating the link.
- Windows support distinguishes filesystem links/junctions from shell `.lnk` shortcuts and explains
  privilege or developer-mode requirements before execution.
- Link creation uses an operation-engine job with destination conflict handling, audit history,
  cancellation where meaningful, and HTTP/Tauri parity.
- Remote providers expose the action only when their capabilities define link creation semantics.
- Invalid targets, privilege failures, destination races, cycles, and unsupported providers produce
  typed errors.
- Tests cover file/directory links, relative targets, Unicode, conflicts, privilege denial, remote
  capability gating, and symlink-safe cleanup.

## Implementation Notes

- Split from the candidate table in 0129; update that parent task when this feature is completed.
- Do not treat `.lnk`, NTFS junctions, and symbolic links as interchangeable abstractions.
- Add a VFS capability only where providers can implement it consistently.

## Agent Notes

- 2026-08-28: Promoted from 0129 into a standalone task during the product feature review.
