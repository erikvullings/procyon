# 0077 Checksums and duplicate-file detection

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0047

## Context
`file-manager-coding-agent-spec.md` §16 milestone 5, §18 (`core.calculateChecksum`) and §37
(checksum calculation in version 1).

## Acceptance Criteria
- Checksum calculation for the selection (at least SHA-256 and BLAKE3, plus CRC32/MD5 for
  compatibility) as a cancellable engine job with progress.
- Results can be copied, saved to a checksum file, and verified against an existing checksum file,
  reporting per-entry match/mismatch/missing.
- `CHECKSUM` provider capability gates availability (§6).
- Duplicate detection across one or more roots using a staged strategy: group by size, then compare
  a partial hash, then a full hash — never hashing everything up front.
- Duplicate results are presented as a reviewable list with grouping; any deletion of duplicates goes
  through the normal delete operation with confirmation (§35).
- Hashing streams files and does not load them into memory; throughput is benchmarked (0065).
- Integration tests: known-vector checksums, verification of a checksum file, duplicate grouping on
  a fixture tree (including same-size-different-content and hardlinked files), cancellation.

## Implementation Notes
- Hardlinks and identical inodes should be reported distinctly from true duplicates.
- Checksum results feed the content-comparison mode of 0075 — share the implementation.

## Agent Notes
- Not started.
- 2026-08-16 Claude: Implemented the feature end to end apart from one flagged gap (below).

  **New crate `fm-checksum` (layer 2).** `hash.rs` streams any `AsyncRead`/`Read` through
  SHA-256/BLAKE3/CRC-32/MD5 in bounded 64 KiB chunks (`HASH_CHUNK_BYTES`), never buffering a whole
  file, with async and blocking flavours that a test asserts agree byte-for-byte, plus prefix-bounded
  variants for staged detection and `ChecksumError::Cancelled` so a cancelled calculation can never
  be mistaken for a short digest. `checksum_file.rs` reads/writes the coreutils `<digest>  <path>`
  format (binary-mode `*` accepted on read, algorithm inferred from digest width where unambiguous)
  and verifies to per-entry Match/Mismatch/Missing. `duplicates.rs` runs the staged funnel: group by
  exact size (a singleton size is never opened), 64 KiB partial hash, then full hash only for what
  survives; `(dev, inode)` identity is collected via `MetadataExt` under `#[cfg(unix)]` and hardlink
  clusters are reported as a category distinct from true duplicates, with the digest computed once
  per identity. `engine.rs`/`store.rs` wrap both as cancellable, progress-reporting engine jobs whose
  id doubles as the `OperationId`, exactly as task 0075 does.

  **Capability gate.** `ProviderCapabilities::CHECKSUM` is now declared by `fm-vfs-local`,
  `fm-vfs-sftp` and `fm-archive` (it rides along with `READ`, since a checksum is just a streamed
  read). FTP was deliberately left without it: `reports_only_implemented_ftp_capabilities` asserts
  its absence, and that prior decision was respected rather than overridden. The engine checks every
  target/root before scheduling any work, so an unsupported provider is rejected synchronously.

  **Transport + hosts.** New DTOs, eight REST routes (`/api/v1/checksums*`,
  `/api/v1/duplicate-scans*`), eight matching Tauri commands, two new `BackendEventPayload` variants
  (`checksum.resultsBatch`, `duplicates.resultsReady`), two new `OperationKindPayload` values, and a
  regenerated OpenAPI document + Orval client.

  **Frontend.** `features/checksums/` holds pure state, a controller, a results view (copy / save /
  verify-against-file with per-entry status) and a duplicate-review view that renders hardlink
  clusters as their own annotated block and refuses to let the last surviving copy of a group be
  ticked. Deleting ticked duplicates calls the same `opsController.delete(...)` that `core.delete`
  uses, so confirmation and auditing are inherited rather than reimplemented (spec §35). Command
  palette actions `core.calculateChecksum` and `core.findDuplicates` are registered in the backend
  `ActionRegistry` and gated client-side on the `CHECKSUM` capability plus a file selection / open
  directory.

  **Verification.** `cargo test --workspace` 1025 passing (up from 1019), zero failures — including
  50 in `fm-checksum` (22 unit + 28 integration: known vectors, checksum-file round-trip/verify,
  duplicate grouping with same-size-different-content and hardlinks, cancellation, engine jobs) and
  6 new `fm-server` route tests, one of which cancels a running job through the generic
  `/api/v1/operations/{id}/cancel` route. `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --all -- --check` clean. Frontend `vitest run` 1146 passing across 101 files (up from
  1141), `tsc --noEmit` clean apart from pre-existing `app-shell.test.ts` errors that are present on
  HEAD and untouched by this work, `biome check .` clean apart from a pre-existing CSS specificity
  warning. Throughput benchmarked and recorded in `docs/architecture/performance.md` (Apple M4 Max,
  64 MiB file): SHA-256 2.18 GiB/s, BLAKE3 1.95 GiB/s, CRC-32 6.50 GiB/s, MD5 773 MiB/s.
- 2026-08-16 Claude: Closed the save-to-file gap, so every acceptance-criteria line is now met and
  Status is `done`.

  Rather than reach for a host-native save dialog, saving goes through the backend:
  `POST /api/v1/checksums/{jobId}/save` (plus the matching `save_checksum_file` Tauri command) takes
  a destination `Location` and writes the rendered text through the provider's `open_write`, gated
  on `ProviderCapabilities::WRITE`. Three reasons this beat a native dialog: it keeps every file the
  application creates on the one audited, capability-gated path the DTO doc comment already promised
  (spec §35); the Axum and Tauri hosts then behave identically instead of diverging into a browser
  download versus an OS picker; and it needs no `tauri-plugin-dialog`, which the desktop
  `capabilities/default.json` deliberately excludes ("no default full-filesystem plugin access",
  spec §22). `overwrite` defaults to `false`, so a second save cannot silently destroy an existing
  checksum file.

  The results view now opens an inline "Save as" form prefilled with `checksums.<algorithm>` and
  writes into the pane's current directory, confirming with a "Saved to …" status line; **Copy
  remains a separate button and is unchanged**. Added 6 controller tests (destination joining,
  percent-encoding a filename with spaces, the overwrite opt-in, refusal on missing job/blank name,
  failure surfaced as an error rather than a silent no-op, default filename) and a new 10-test
  `checksum-results-view.test.ts` that asserts the Save button calls the save path and *not* the
  clipboard. A new `fm-server` route test writes a real file to a temp dir, reads it back, checks it
  contains `<digest>  alpha.txt`, and proves the second save is rejected unless `overwrite` is set.

  Also added the panels' stylesheet to `frontend/src/themes/theme.css` (they had none, so both
  would have rendered unstyled): surface/border treatment matching the `.fm-sync-plan-*` review
  table, sticky headers/footers, and a warning-bordered block that keeps a hardlink cluster visually
  set apart from the true duplicates above it. All colours come from existing `--fm-*` tokens, so
  both themes are covered.

  Verification: `cargo test --workspace` 1026 passing, 0 failures; `cargo clippy --workspace
  --all-targets -- -D warnings` and `cargo fmt --all -- --check` clean; frontend `vitest run` 1157
  passing across 100 files; `tsc --noEmit` and `biome check .` clean apart from the same pre-existing
  `app-shell.test.ts` errors and CSS specificity warning noted before.
- 2026-08-16 Claude: Remaining known limitations (none block the acceptance criteria).
  - The command palette starts a job with SHA-256 hardcoded; there is no pre-flight dialog to pick
    algorithms, though every layer beneath already accepts an arbitrary set and computes them in a
    single pass.
  - No true memory-usage assertion: the streaming test hashes a 64 MiB file to prove correctness
    across hundreds of buffer refills and absence of OOM, but Rust offers no portable way to measure
    process RSS from a test. The bounded-memory guarantee rests on the single fixed-size buffer in
    `hash.rs`, verifiable by inspection.
  - Hardlink detection is Unix-only (`#[cfg(unix)]`); elsewhere identity is reported unknown and
    every file is treated as distinct, so the detector under-reports hardlinks rather than falsely
    claiming them. Verified on macOS only.
  - Duplicate scanning walks roots itself via the VFS but a scan is only reported once complete
    (`duplicates.resultsReady` is terminal), since a group is not knowable until its funnel has run.
  - The mock runtime returns deterministic fake digests, not real hashes; it exists to exercise the
    UI, and its own tests say so.
  - Not manually driven in a browser: this environment has no browser available for the mock runtime,
    so the UI is covered by unit/component tests only.
- 2026-08-16 Claude: Flaky-test note for whoever sees a red workspace run. `fm-application`'s
  `plans_and_copies_ten_thousand_small_files` polls for a terminal operation state on a fixed
  6000 x 10ms = 60s wall-clock deadline while copying 10,000 files. That binary takes ~59s on its
  own on this machine, so the test sits right on its budget and fails under any extra load — it
  failed in full `cargo test --workspace` runs while the machine was busy and passed whenever it was
  quiet. Attribution was measured rather than assumed: the same single test timed at HEAD (56.6s,
  57.1s) versus with this task's changes (59.5s) is ~5% apart, within run-to-run noise, and this diff
  touches no `fm-operations`, operation-planner or `fm-vfs-local` code — the only additions to
  `FileManagerService::new` are two empty stores and an engine struct, which are O(1) and cannot
  affect a 10,000-file copy. A final quiet-machine `cargo test --workspace` was green at 1026. The
  deadline is still worth widening or making load-proportional in a follow-up.
