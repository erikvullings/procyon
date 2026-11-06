# 0302 Optional post-transfer verification

Status: done
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
- 2026-11-06 claude: Opt-in per copy. `StartOperationRequestDto.verifyTransfer` (default false; honoured for `copy` and `duplicate`, ignored for other kinds) sets `Operation.verify_transfer`. `CopyExecutor::copy_file` compares the FINISHED PRIVATE copy with its source before `commit_copy` publishes it (`compare_copy`: both sides streamed in 128 KiB chunks, constant memory, cancellable and pausable between chunks, first differing byte reported). Outcomes per file: `verified`, `mismatch`, `notVerified` (a provider cannot be read back: `UnsupportedCapability`), `skipped` (symbolic links). Counts live in `Operation.verification` and reach clients through `OperationDto.verification` and the `operation.*` event payload.
- Mismatch handling: the item fails as an entry-scoped warning with code `verificationMismatch` (params: name, sourceSize, copySize, offset = the evidence), the private partial is discarded, and NOTHING is published, so a destination that fails verification never looks complete; other items continue and the job ends `completedWithWarnings`. Read errors during verification are ordinary provider errors (and therefore go through the 0301 retry when transient).
- UI: a "Verify copied files" checkbox in the copy confirmation dialog (remembered per viewer in localStorage, `verify-preference.ts`; the controller adds `verifyTransfer` to copy requests when it is on), a verification line on the operation card, red when anything differed, and 8-locale strings (`verifyOption`, `verifyOptionHint`, `verificationSummary`, `verificationMismatch`). Existing dialog/app-shell tests that asserted the exact Tab order now expect the checkbox between the two buttons.
- Verified: 7 new planner tests (identical copy, differing copy not published with evidence and no leftover partial, unreadable source published but `notVerified`, first difference across chunk boundaries, truncated/extended/empty copies, cancellation), 1 service-level test (summary in the operation DTO, absent when not requested), 5 frontend tests plus 1 controller test; `cargo clippy` clean, `cargo nextest run -p fm-operations -p fm-events -p fm-application -p fm-transport-dto -p fm-server` 1717 passed, frontend suite green. Also fixed a type error in the 0301 `transfer-retry-text.test.ts` (params must be strings) found by `tsc`.
- DEVIATIONS from the acceptance text: (1) no provider-native checksum shortcut (S3 ETag, WebDAV, SFTP hash): every provider is verified by streaming read-back, because none of them exposes a content checksum through the VFS trait yet and ETags are not content hashes for multipart uploads; verifying costs a second read of both sides. (2) Not offered for moves: a move's source is deleted, so a safe check would have to sit between copy and delete (see 0303 notes); the checkbox shows for copies only. (3) `ChecksumCoordinator` is not reused: it hashes whole entries for the checksum feature, whereas a byte comparison reports the first differing offset and needs no hash. (4) The restart of an interrupted transfer (0300) does not carry the verify flag (not journaled). (5) No visual check of the new checkbox in a real window.
- Known gaps: `notVerified` is a count only (no per-file list); the live card updates on the completion event, not per file; the preference is a per-viewer localStorage value, not a Settings entry.

