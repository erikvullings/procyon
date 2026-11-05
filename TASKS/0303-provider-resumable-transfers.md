# 0303 Provider byte-range and multipart resume

Status: open
Priority: medium
Subsystem: backend, remote
Depends on: 0299, 0300, 0301

## Context
Resume is a capability, not a promise. Providers that can prove a partial destination belongs to
the same job and source revision may continue instead of restarting. Parent: 0163.
Currently only OneDrive advertises `resumable_upload/download`; SFTP, WebDAV, S3, FTP do not.

## Acceptance Criteria
- Each provider that claims resume implements and tests it: SFTP (offset write on a job-owned
  partial), S3 (multipart upload ids with listed parts), WebDAV (ranged GET; PUT resume only where
  the server provably supports it); others stay `false`.
- Resume requires proof: the partial is job-owned (journaled name/upload id) and the source
  revision matches; otherwise the transfer restarts or needs intervention.
- Expired credentials, replaced partial, and stale source fall back safely and report why.
- Fault-injection tests per provider: disconnect mid-transfer, replaced destination, stale source,
  expired credentials, multipart recovery.
- `TransferCapabilities` advertisement matches implemented behaviour (no over-advertising).

## Implementation Notes
- Split per provider into separate commits; land the shared proof/validation logic first.

## Agent Notes
- 2026-11-05 claude: Split from 0163. Needs the journal (0299) and classification (0300).
