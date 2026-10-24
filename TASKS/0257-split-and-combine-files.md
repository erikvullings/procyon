# 0257 Split and combine files

Status: done
Priority: low
Subsystem: backend, frontend
Depends on: 0035, 0077

## Context
Total Commander File > Split/Combine file: split a large file into fixed-size parts (with a CRC/manifest) and re-join parts. Useful for FAT32 limits, uploads, and email. Not present today.

## Acceptance Criteria
- "Split file…" dialog: part size presets/custom, target folder, naming (`name.001`, …) and optional checksum manifest; runs as a cancellable job in `fm-operations`.
- "Combine files…" on `.001/.002…` selection (or one part): detects the set, verifies manifest if present, rejects gaps, joins into the target.
- Works across VFS providers via streaming (no full-file buffering); conflict policy via existing dialog (0045).
- Never overwrites silently; temp-root tests for destructive paths.

## Implementation Notes
- New operation kinds in `fm-operations`/`OperationsCoordinator`; reuse transfer planning for cross-provider targets.
- Manifest format documented in docs; keep simple (`.crc`/BLAKE3 list).

## Agent Notes
- Created 2026-10-18 from the feature gap review. No work started.
- 2026-10-24 claude: Implemented end-to-end (Rust + frontend, mock/HTTP/Tauri parity).
  - **Operation engine**: new `OperationKind::SplitFile` / `CombineFiles` (`fm-application/src/split_combine.rs`,
    pure naming/set-detection/manifest rules in `split_manifest.rs`). Streams 256 KiB chunks through the provider
    traits (source and target may be different providers), BLAKE3 per part + whole file via `blake3`. Parts and
    output go through private temp siblings + `commit_copy` (never replaces unless the user chose Overwrite);
    conflicts use the existing dialog (probe: first/last part, manifest, stale next part). Failure/cancel removes
    parts already published by the failing item. Undo = `RemoveCreated` with known hashes (unavailable after an
    Overwrite). Overwriting a split deletes stale higher-numbered parts of the replaced set.
  - **Combine**: set detected from any selected part by listing the folder; gaps and mixed selections rejected at
    plan time; manifest (`<name>.blake3`, documented in `docs/architecture/file-operations.md`) checked for part
    count/names, per-part hash, total size and whole-file hash before the output is published. Verification
    mismatches are reported as an entry warning (the engine hides plain `Failed` reasons) with nothing written.
  - **Transport**: `StartOperationRequestDto.splitFile` (all literals updated); `POST /api/v1/split/inspect`
    (`inspectSplitSet`) + Tauri `inspect_split_set`; OpenAPI + generated client regenerated.
  - **Frontend**: `client.splitFile` / `client.combineFiles` actions (context menu + registry), `SplitFileDialog`
    (presets 10 MiB..FAT32 4 GiB-1, custom size/unit, part count preview, manifest toggle, target same folder or
    other pane), `CombineFilesDialog` (detects set, manifest note, gap/damage reasons, editable output name);
    `inspectSplitSet` on mock/HTTP/Tauri clients; controller `splitFile`/`combineFiles`; i18n in all 8 locales.
  - **Verified**: `cargo nextest run -p fm-application --test split_combine_operation` 11 tests (round trip across
    folders, single/empty file, undo, conflicts ask/skip/rename/overwrite + stale parts, gaps, damaged part +
    manifest, interleaved sets, invalid requests, cancel); `--lib split_manifest` 8; `fm-server --test files_routes`
    +1; frontend `split-actions` 5, `split-dialogs` 6, controller +2, mock client +1.
  - **Known gaps**: no cross-provider (e.g. SFTP) integration test, streaming is exercised on the local provider
    only; without a manifest a missing *last* part is undetectable; the cancel test only asserts cleanup when the
    cancel lands before the job finishes; no CRC32 variant (BLAKE3 manifest only); only the context menu / action
    registry expose the commands (no dedicated main-menu entry).
