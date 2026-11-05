# 0302 Optional post-transfer verification

Status: open
Priority: medium
Subsystem: backend, operations, frontend
Depends on: 0035, 0047

## Context
Users want confidence that a completed transfer is byte-correct. Parent: 0163.

## Acceptance Criteria
- An opt-in per-operation setting verifies each completed item using a provider-appropriate
  method: checksum when the provider offers one (e.g. S3 ETag/checksum, WebDAV, SFTP hash
  extension), otherwise read-back comparison; unsupported combinations report `notVerified`.
- Verification status (`verified`, `mismatch`, `notVerified`, `skipped`) is recorded per item and
  summarised in operation history, DTOs, and the UI.
- A mismatch fails the item, keeps the evidence, and never leaves a destination that looks
  complete and is not.
- Verification is cancellable and bounded; it reuses `ChecksumCoordinator` where applicable.
- Tests: match, mismatch, unsupported provider, cancellation during verification.

## Agent Notes
- 2026-11-05 claude: Split from 0163. Independent of journal and retry.
