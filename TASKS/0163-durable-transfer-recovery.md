# 0163 Durable transfer recovery

Status: open
Priority: high
Subsystem: backend, operations, remote
Depends on: 0035, 0047, 0108

## Context

Long local and remote transfers can be paused during one process lifetime, but users need confidence
that an application crash, restart, disconnect, or expiring remote session will not force an
unexplained restart or leave ambiguous partial files.

## Acceptance Criteria

- Queued and running transfer jobs persist enough state to recover safely after process restart.
- On recovery, each job is classified as resumable, safely restartable, completed-but-unconfirmed, or
  requiring user intervention, with the reason shown.
- Providers advertise resumable read/write capabilities; byte-range or multipart resume is used only
  when the provider can prove the partial destination belongs to the same job and source revision.
- Completed transfers can optionally be verified with a provider-appropriate checksum or read-back,
  and verification status appears in operation history.
- Private partial destinations are never exposed as completed files and can be deliberately retried
  or removed.
- Retry uses bounded exponential backoff and surfaces authentication/host-key/conflict errors rather
  than retrying them indefinitely.
- Tests fault-inject restart, disconnect, stale source, replaced destination, expired credentials,
  multipart recovery, checksum mismatch, and cancellation during recovery.

## Implementation Notes

- Persist intent and checkpoints, not live provider/session objects. Reconnect through the existing
  connection facade and credential store.
- Define crash-consistent journal writes before changing production transfer behavior.
- Resume is a capability, not a promise: conservative restart or explicit intervention is preferable
  to appending bytes to an unverified destination.

## Agent Notes

- 2026-08-28: Created from the product feature review. This is especially valuable for SFTP, WebDAV,
  and S3 transfers and should preserve the operation engine as the sole mutation authority.
