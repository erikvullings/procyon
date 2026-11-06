# 0315 S3 multipart upload resume

Status: open
Priority: low
Subsystem: backend, remote
Depends on: 0303

## Context
Split from 0303. The engine side of resuming (journaled partial hints, a byte-for-byte proof
that the partial is a prefix of the current source, fall back to a full copy and say why) exists
and is exercised by SFTP. S3 cannot reuse the SFTP mechanism: an object has no appendable
partial. An interrupted upload is a multipart upload (`UploadId`) with some parts committed,
and nothing is visible at the destination until `CompleteMultipartUpload`.

## Acceptance Criteria
- The S3 provider writes private temporaries as multipart uploads and exposes the upload id and
  committed parts to the engine so they can be journaled with the partial.
- A resume lists the parts (`ListParts`), proves each part's size (and ETag/checksum where the
  bucket returns one) against the same byte range of the current source, uploads only the
  missing parts and completes the upload.
- An expired, aborted or foreign upload id falls back to a full copy with the recorded reason;
  the abandoned upload is aborted (`AbortMultipartUpload`) so it does not keep costing storage.
- `TransferCapabilities` advertises `resumable_upload` only once this works.
- Fault-injection tests against a fake S3 endpoint: interrupted upload, missing part, changed
  source, aborted upload id, expired credentials.

## Implementation Notes
- The journal's `Partial` record stores a location; it will also need the provider-specific
  resume token (the upload id). Extend it additively.
- Do not treat a multipart ETag as a content hash.

## Agent Notes
- 2026-11-06 claude: Split from 0303, which delivered the shared machinery and SFTP only.
- 2026-11-06 claude: Live check against SeaweedFS (podman): ranged `GetObject` works, so S3 DOWNLOAD resume already works through the 0303 machinery; upload still restarts from the beginning, as designed here. Also fixed the writer race this exposed (see 0303): an S3 upload writer now reports the upload's verdict from `shutdown`.

