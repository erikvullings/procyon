# 0163 Durable transfer recovery

Status: done
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
- 2026-11-05 claude: Too large for one change (no journal, retry, verification or non-OneDrive resume exists). Split into 0299 journal, 0300 recovery classification, 0301 retry/backoff, 0302 verification, 0303 provider resume. This task is the umbrella: mark done when 0299-0303 are done.
- 2026-11-06 claude: 0299-0303 are done. 0303 delivered the shared resume machinery and SFTP only; S3 multipart resume (0315) and WebDAV resume (0316) are split out. This umbrella closes when those are done.
- 2026-11-06 claude: DONE. 0299-0303 delivered the journal, classification, retry, verification and the shared resume machinery with SFTP; 0315 added S3 multipart upload resume (seal + `UploadPartCopy`), 0316 WebDAV ranged reads (upload keeps restarting, reported), 0317 FTP completion reporting and `APPE` resume. Live-verified against OpenSSH, SeaweedFS and `hacdias/webdav` (podman, removed afterwards); FTP is fixture-tested only. Remaining known limits are in the Agent Notes of 0303, 0315, 0316 and 0317.
