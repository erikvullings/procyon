# 0315 S3 multipart upload resume

Status: done
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
- 2026-11-06 claude: DONE. Design differs from the sketch above in two deliberate ways. (1) The engine proves a partial by reading it back, and an unfinished multipart upload cannot be read, so an interrupted upload is SEALED: `CompleteMultipartUpload` over its contiguous committed parts (any prefix is valid: every part but the last is >= 5 MiB) turns it into a regular object the unchanged engine proof (`prove_resume`, byte-for-byte vs the CURRENT source) can read. This is stronger than comparing part sizes/ETags and never treats a multipart ETag as a hash. (2) The upload id is not journaled; it lives in a marker object `<temporary>.fm-upload` next to the temporary (an unfinished upload is invisible in listings), so S3 needed no journal change and survives a crash. Resume (`open_write_at`) starts a NEW upload whose first part(s) are `UploadPartCopy` of the sealed object (server-side, <= 4 GiB each) and uploads only the rest, then completes over the same key. Only private temporaries (`.fm-copy-*`) take part: their interrupted upload is kept instead of aborted. `inspect` of such a temporary seals first; `discard_copy` aborts the upload and removes the marker (no storage leak); a marker whose upload expired/was aborted is dropped and the copy restarts. Caps: `resumable_upload` and `random_write` true.
- Files: `crates/fm-vfs-s3/src/provider.rs` (marker, seal, `resume_upload`, `copy_ranges`), `fixture.rs` (ListParts, UploadPartCopy, unique upload ids), `tests/provider_contract.rs`.
- Verified: 7 new tests in `provider_contract.rs` + 3 unit tests (interrupted upload sealed and resumed byte-for-byte, wrong offset refused, discard aborts, expired marker dropped, only temporaries resume, range splitting, XML decode); LIVE against SeaweedFS (podman): the multipart upload resumed from its 5 MiB part and verified (`live_s3_resumes_downloads_and_multipart_uploads`). The live run caught a real bug the fixture could not (SeaweedFS escapes ETag quotes as `&#34;`), fixed.
- Limits: a crash between `CreateMultipartUpload` and writing the marker leaks an upload with no marker (cost only; bucket lifecycle rule `AbortIncompleteMultipartUpload` is the usual remedy). `inspect` of a temporary must not run while a writer for it is still uploading (it would seal it); the engine never does. A resume needs >= 5 MiB committed (a single `PutObject` upload below the multipart threshold has nothing to continue and restarts). Expired credentials surface as a permission error from the seal/list call and fail the proof as unverifiable, restarting the copy.
