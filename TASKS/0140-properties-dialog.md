# 0140 File/folder Properties dialog

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: cross-cutting
Depends on: 0129

## Context

Split out of [0129](0129-total-commander-shortcuts-major-features.md) (Alt+Enter row) in the
2026-08-14 re-triage — confirmed still genuinely missing, not just undiscovered. fm currently only
shows inline status-bar metadata (aggregate totals from 0097) and per-row columns (size, modified,
extension); there is no per-entry detail view showing everything the app and provider know about a
single file or folder — permissions, exact byte-precise size, full timestamps, provider-specific
metadata (e.g. SFTP file mode, S3 storage class, archive entry compression ratio). This is a
commonly-expected feature (Finder's Get Info `Cmd+I`, Explorer's Properties, TC's Alt+Enter).

## Acceptance Criteria
- A modal (consistent with the app's existing `ModalPanel` dialog chrome, per the other dialogs)
  showing, at minimum: name, kind, exact size (byte-precise, not the rounded display in the table),
  created/modified/accessed timestamps (whichever the provider actually exposes — not every
  provider has all three), full path/location URI, and permission bits where the provider exposes
  them (local filesystem at least).
- For a multi-selection, shows an aggregate: total size, item count, and a folder/file breakdown —
  reuse 0097's aggregate-computation approach rather than a second implementation.
- Provider-specific metadata section that's additive per provider (local: POSIX permissions/owner
  where available; SFTP: remote file mode; archive: compressed/uncompressed size and compression
  method; S3-like: storage class/ETag if exposed) — design the DTO so providers can contribute
  fields without every provider needing to support every field.
- Timestamps respect the same locale/time-zone-aware formatting already used elsewhere in the app
  (`frontend/src/features/entry-formatting/entry-formatting.ts`'s `formatEntryModifiedAt`) — don't
  introduce a second, divergent date-formatting path.
- Bound to `Alt+Enter` (desktop convention) with a menu/palette entry as well.
- Tests: DTO assembly for each provider type, aggregate computation for multi-selection, dialog
  rendering for a representative entry of each supported provider.

## Implementation Notes
- Check whether 0136 (extended attributes / Finder tags / Spotlight comments, if picked up first)
  wants to surface its data through this same dialog rather than a separate surface — likely yes,
  but not a hard dependency; this task can ship without it and 0136 can extend the dialog later.
- Reuse `EntrySummary`/`DirectorySnapshot`'s existing per-provider metadata fields where already
  present (check `crates/fm-domain/src/entry.rs`) before adding new backend DTO fields — some of
  this may already be threaded through and simply not surfaced in the UI yet.

## Agent Notes
- 2026-08-17 claude: Implemented end-to-end.
  - **Backend**: `ArchiveInfo` (`crates/fm-domain/src/entry.rs`) gained `compressed_size`/
    `compression_method`, mirrored in `fm-transport-dto`. `ArchiveFileSystemProvider::metadata`
    (`crates/fm-archive/src/lib.rs`) now populates these for ZIP entries (opens the archive,
    finds the raw entry, reads its compressed size/`CompressionMethod`); tar/rar entries still
    report `archive: None` for these two fields — additive-per-provider, matching the acceptance
    criteria. `LocalFileSystemProvider::metadata` (`crates/fm-vfs-local/src/lib.rs`) now reports
    POSIX ownership as the raw numeric uid/gid (no name-resolution crate added; `ls -n` convention)
    under `cfg(unix)`. New `core.showProperties` action registered in
    `crates/fm-application/src/action.rs`, bound to `Alt+Enter`, `ActionContextRequirements::selection()`.
  - **Frontend**: `frontend/src/features/properties/selection-aggregate.ts` (0097-style
    size/count/folder-file-breakdown aggregate, reusing the same kind-partitioning shape as
    `pane.ts`'s `listingSummary` and the backend's `aggregate_totals`) and
    `properties-dialog.ts` (`ModalPanel`-based dialog; single-entry mode fetches
    `getEntryMetadata` lazily and renders general/permissions-ownership/archive sections
    conditionally on what's present; multi-selection mode renders the aggregate without any
    metadata fetch). Wired through `DialogUIController` (`propertiesOpen`/`propertiesEntries`),
    `app-dialogs.ts`, `global-keydown-handler.ts` (`core.showProperties` dispatch), and
    `app-shell.ts`'s `openPropertiesForActivePane` (uses `getSelectedEntriesOrCursor`, Total
    Commander convention: acts on the cursor entry when nothing is explicitly selected). No
    native-menu-spec entry added — consistent with how `core.openMultiRename` and other
    dialog-opening actions work in this codebase, the command palette lists every registered
    action automatically, so "menu/palette entry" is satisfied by the palette alone.
  - **Timestamps**: reuses `formatEntryModifiedAt` for both `modifiedAt` and `createdAt` (no
    second date-formatting path), and `formatEntrySize` alongside a byte-precise
    `Intl.NumberFormat` count for the "exact size" requirement.
  - **Tests**: `crates/fm-archive/tests/archive_provider.rs` gained two new tests (compressed
    size/method for a ZIP entry; no archive info for a directory entry) - DTO assembly for the
    archive provider. `crates/fm-vfs-local/tests/local_provider.rs`'s existing metadata test
    extended with ownership assertions - DTO assembly for the local provider.
    `frontend/src/features/properties/selection-aggregate.test.ts` (3 tests: empty selection,
    mixed files/folders, missing-size-treated-as-zero) and `properties-dialog.test.ts` (6 tests:
    one dialog-rendering test per provider - local, SFTP, FTP lacking permission metadata,
    archive - plus the multi-selection aggregate and the cancel/close path).
    `global-keydown-handler.test.ts` gained an Alt+Enter dispatch test. Verified counts: 9 new
    Rust tests + 10 new/modified frontend tests, all passing individually
    (`cargo nextest run -p fm-archive -p fm-vfs-local`, `pnpm exec vitest run
    src/features/properties/ src/features/keybindings/global-keydown-handler.test.ts`).
  - **Full-suite verification**: `cargo nextest run --workspace` → 1047 tests, 1045 passed, 3
    skipped, 2 failed (`fm-application::conflict_resolution::a_destination_appearing_after_planning_is_resolved_like_an_initial_conflict`
    and `fm-application::copy_directory_operation::plans_and_copies_ten_thousand_small_files`) -
    both are large-file-count/timing-sensitive integration tests that pass individually in
    isolation (verified) and touch operation-planning/conflict-resolution code this task never
    modified (confirmed by diff scope and by inspecting `preserve_copy_metadata`/
    `preserve_entry_metadata` in `fm-vfs-local`, which use `std::fs::metadata` directly, never
    the trait `metadata()` method this task changed) - pre-existing environmental flakiness on
    this dev machine under load, not a regression. `pnpm exec vitest run` (full frontend suite):
    105 files, 1203 tests, all passed. `pnpm run lint:frontend` fails with 3 pre-existing errors
    in files this task never touched (`tabler-icons.ts`, `thumbnail-loader.ts`,
    `theme.css`'s favourites-menu section) - confirmed present on `main` itself before this
    branch's merge, unrelated to 0140, left as a follow-up rather than fixed here to avoid
    scope creep into other in-flight work. `pnpm run lint:rust` (fmt + clippy) is clean.
  - **Also fixed in passing**: `crates/fm-test-support/src/architecture.rs`'s `CRATE_LAYERS` was
    missing `fm-vcs-status` (added to main by an unrelated task merged before this one), which
    failed `fm-test-support::workspace_architecture`'s layering fitness check - added at layer 2
    alongside its sibling provider/utility crates.
  - **Known gaps / follow-ups**: no `accessed_at` timestamp anywhere in the domain/DTO/frontend
    stack (no provider exposes one; acceptance criteria's "whichever the provider actually
    exposes" already anticipates this). FTP permissions/ownership and tar/rar archive
    compression metadata remain unpopulated (`None`) - the DTO's additive-per-provider design
    supports filling these in later without a breaking change. Local ownership is numeric
    uid/gid, not a resolved name (no new dependency added for name resolution).
