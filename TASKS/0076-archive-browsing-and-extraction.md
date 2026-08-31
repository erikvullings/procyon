# 0076 Archive provider: browse, mutate and passwords

Status: done
Priority: medium
Owner: unassigned
Agent: codex
Area: backend
Depends on: 0047

## Context
`file-manager-coding-agent-spec.md` §16 milestone 5, §6 (archive provider is the first designed-for
future provider) and §37 (archive browsing for common formats in version 1). Users should be able
to enter an archive and work with its contents like another folder, including password-protected
archives. Archive parsing, compression and encryption must come from maintained, permissively
licensed open-source libraries (MIT, Apache-2.0, BSD or similarly acceptable); preserving that
licensing constraint is more important than complete format or feature coverage. This project must
not implement archive codecs or cryptography.

## Acceptance Criteria
- `fm-archive` implements a `FileSystemProvider` for the `archive://` scheme so archives are
  navigable as directories (`archive:///path/to/example.zip!/docs`) using the existing panes and
  table with no UI special-casing (§5.1, §6).
- A documented, tested format/capability matrix evaluates at least ZIP, 7z, RAR and tar archives
  (including gzip/bzip2/xz compression where applicable). Detection uses content where practical,
  not only the filename extension, and provider capabilities truthfully reflect each format and
  backend. ZIP and 7z are required targets; RAR is best-effort and may offer only the features
  available from acceptable open-source dependencies.
- Opening an archive, navigating nested directories, viewing metadata and opening/copying an entry
  out of the archive work through the existing pane, VFS and operation-engine paths in both Axum
  and Tauri hosts.
- Copying files or directory trees from the local filesystem into a writable archive, copying
  entries between writable locations inside an archive, and deleting files or non-empty
  directories inside a writable archive run as normal engine operations with progress, safe-point
  cancellation and conflict handling. Multi-selection uses the existing operation semantics.
- Archive mutation is transactional: write a validated replacement archive beside the original,
  flush it, then atomically replace the original where the host filesystem supports that. Failure
  or cancellation preserves the original byte-for-byte and removes temporary output. Never assume
  an archive format supports safe in-place edits.
- Formats/backends that cannot be rewritten safely are exposed as read-only with a typed
  unsupported-capability error and disabled mutation actions; support must never be advertised and
  then silently degrade. Limited or absent RAR support is acceptable when broader coverage would
  require a proprietary, source-available or otherwise non-permissive dependency.
- Extraction runs as a normal engine operation with progress, cancellation and conflict handling.
- Archive creation from a selection, with format and compression-level options.
- Security: entry paths are validated so extraction cannot escape the destination
  ("zip slip"), symlink entries are not followed, and absolute paths in archives are rejected —
  covered by explicit tests.
- Resource limits guard against decompression bombs (uncompressed-size and ratio caps) with a clear
  error.
- RAR entry extraction is cached per backend session so repeatedly viewing comic pages does not
  repeatedly decode the same data. The cache is bounded, keyed by the archive's current filesystem
  fingerprint plus inner path, and invalidated when the archive changes.
- Cached plaintext lives in a private session temporary directory. Normal backend shutdown removes
  it; each session holds an OS lock so startup can safely remove unlocked directories abandoned by
  crashed sessions without deleting another running instance's cache.
- Password-protected archives are detected and request a password through an application-level
  credential challenge that has equivalent Axum and Tauri transports. A correct password permits
  every operation supported by that format; missing/wrong passwords return distinct typed errors
  and can be retried without restarting navigation or the operation.
- Passwords are never placed in a `Location`, operation history, persisted workspace/settings,
  event payload, diagnostics, tracing field or error text. Keep them only in memory for the minimum
  useful scope, redact them from logs, and zeroize owned secret buffers where practical. Saving a
  password in an OS credential store is out of scope unless separately designed and approved.
- Integration tests use temporary roots and cover ZIP and 7z plus RAR where the selected dependency
  supports it; nested folders; Unicode names; copy out; copy file/tree in; copy within; delete
  file/tree; cancellation and
  rollback during rewrite; conflicts; correct, missing and wrong passwords; corrupt archives;
  zip-slip; symlink escape; and decompression-bomb guards. Tests for unsupported format mutations
  assert truthful read-only capabilities and typed errors.

## Implementation Notes
- Reading must be lazy: browsing an archive should not extract it to a temp directory wholesale.
- Start with a dependency/licensing spike and commit its decision to an ADR. Require an OSI-style
  open-source, permissively licensed implementation, preferably native Rust. Candidate evidence as
  of 2026-08-04 includes the MIT `zip` crate (ZIP read/write plus AES and ZipCrypto) and
  BSD-licensed libarchive (broad ZIP/7z/RAR
  reading, with format-specific limitations). The `unrar` Rust wrapper is MIT/Apache-2.0 but embeds
  code under the more restrictive UnRAR license, so do not select that backend unless a separate
  licensing review establishes that it meets this task's open-source policy. Verify
  transitive licenses, maintenance, supported encryption variants, streaming/random-access
  behavior, memory safety, and Windows/macOS/Linux packaging before selection; do not shell out to
  an installed archiver or require users to install one.
- Treat an archive plus its inner path as one provider location. Normalize inner paths before I/O,
  reject `..`, absolute paths and platform-prefix escapes, and define behavior for duplicate entry
  names and nested archives. Entering a nested archive recursively is out of scope unless added by
  an explicit follow-up decision.
- Reuse the provider-neutral copy/delete executors rather than creating archive-only application
  logic. If the current VFS write/remove contracts cannot express transactional archive rewrites or
  credential challenges, extend them minimally and document the contract for future providers.
- This is the second real provider — expect it to expose any local-filesystem assumptions leaked
  into the engine, and record them in the notes.

## Agent Notes
- 2026-08-04 codex: Expanded the existing matching task for the requested folder-like archive
  support instead of creating a duplicate. Added ZIP/7z/RAR coverage, copy/delete inside archives,
  transactional rewrites, password handling and secret-lifetime rules, host parity, licensing due
  diligence, capability honesty and regression/security tests. Implementation has not started.
- 2026-08-04 user clarification: Open-source licensing takes precedence over complete RAR coverage;
  missing RAR features are acceptable and must be represented honestly in the capability matrix.
- 2026-08-04 user decision: Cache archive passwords in backend memory for the lifetime of the
  backend session; never persist them.
- 2026-08-04 codex: Implemented the first provider slice: safe `archive://...!...` locations,
  content-signature detection and lazy directory browsing for ZIP/7z, entry reads, ZIP
  transactional add/create-directory/delete rewriting, session-only zeroizing password storage,
  Axum/Tauri credential submission, typed credential/security/resource-limit errors, and provider
  registration shared by both hosts. Added six focused provider tests plus archive-location tests;
  verified `fm-archive`, `fm-application`, `fm-server`, and `fm-desktop` compile. Remaining before
  this task can be marked done: tar-family browsing, explicit RAR read-only detection/capabilities,
  encrypted fixture/retry tests and frontend challenge UX, directory-tree/within-archive operation
  integration tests, cancellation safe-points during rewrite, archive creation options, extraction
  integration, symlink tests, and full workspace lint/test verification.
- 2026-08-04 codex: Extended the slice with content-detected tar/gzip/bzip2/xz browsing, explicit
  read-only RAR 4/5 handling, encrypted ZIP missing/wrong/correct-password retry tests, corrupt ZIP
  and tar-link rejection tests, cancellation rollback coverage, and a documented format capability
  matrix. The normal provider-neutral operation engine now copies files and directory trees into,
  within and out of ZIP archives and recursively deletes ZIP trees; cross-provider commits no
  longer assume the source has a native path or transferable local metadata.
- 2026-08-04 codex: Added application credential error codes, a backend-session password cache for
  both Axum and Tauri, generated clients, and a masked frontend password challenge that retries the
  same navigation. Passwords remain only in the provider's zeroizing in-memory map and transient
  dialog input.
- 2026-08-04 codex: Task remains in progress. Archive creation from a selection with explicit
  format/compression options is not represented by the current operation DTO and is not yet
  implemented. Password-protected 7z behavior, credential retry for file viewing/operations, and
  explicit engine conflict/multi-selection fixtures also still need coverage before all acceptance
  criteria can be claimed.
- 2026-08-04 codex: Local ZIP, 7z, RAR and tar-family files now enter their `archive://...!/`
  root through the normal entry activation path, so both Enter and double-click show archive
  contents in the existing folder pane. Ordinary files continue through `core.open`; recursively
  entering an archive stored inside another archive remains intentionally out of scope.
- 2026-08-04 codex: Added comic-book archive aliases (`.cbz` and `.cbr`) and standalone `.gz`
  activation. Content detection still selects ZIP versus RAR for comic archives; standalone gzip
  is exposed as one read-only decompressed member, while gzip-compressed tar remains a folder tree.
  Fixed the directory viewport remeasurement after the selection metadata bar changes pane height,
  preventing a stale virtual-body height from creating an unnecessary scrollbar.
- 2026-08-04 codex: Failed archive entry is now transactional from the user's perspective: an
  explicit destination is listed before its location is committed to workspace history. Unsupported
  RAR-backed CBR files therefore report the error without replacing the last usable tab location,
  leaving retry, reload, and breadcrumb navigation operational.
- 2026-08-04 codex: Archive paths now cross the provider boundary correctly. Parent navigation
  leaves an archive root for its containing local directory, while breadcrumbs display the outer
  filesystem path plus archive/inner segments and map each click to either a local or archive
  location as appropriate.
- 2026-08-04 codex: Removed the one-pixel overflow on short archive listings by pinning the
  accessibility cursor announcement inside the directory viewport instead of letting its static
  position fall immediately after the full-height virtual body.
- 2026-08-04 codex: Fixed the remaining short-list scrollbar by clipping the final alternating
  filler stripe to the viewport remainder instead of rendering a full row after rounding its count
  up. A rendered 113px viewport now has equal client and scroll heights.
- 2026-08-04 codex: Added read-only RAR comic browsing and page reads through the pure-Rust,
  MIT/Apache-2.0 `rars` crate, including the existing per-session password flow and archive safety
  limits. Comic aliases remain content-driven: ZIP-backed CBR/CBZ files use `zip`, RAR-backed files
  use `rars`, and ZIP reader probing now accepts legal archives with prepended stubs instead of
  requiring `PK` at byte zero.
- 2026-08-04 codex: Constrained each pane's content grid to its assigned split width so archive
  tables, borders, scrollbars, and the F3 image viewer cannot paint across the adjacent pane. RAR
  page reads now stop decoding immediately after the requested member, while still decoding the
  required prefix for solid archives.
- 2026-08-05 user decision: Cache extracted archive entries per backend session. Keep plaintext in
  a private, bounded temporary cache, remove it on normal shutdown, and reclaim cache directories
  abandoned by crashes on a later backend startup.
- 2026-08-05 codex: Implemented a private RAR extraction cache scoped to the archive provider
  session, bounded to 512 entries/512 MiB with LRU eviction and archive size/mtime invalidation.
  Normal provider drop removes the session directory; an OS lock protects live sessions while a
  later startup removes unlocked crash leftovers. Added two cache lifecycle tests; all 19
  `fm-archive` unit/integration tests pass and its all-target clippy check is warning-free. Full
  workspace verification remains blocked by pre-existing missing `DirectorySnapshot` fields in
  `fm-domain`/`fm-transport-dto`; `pnpm test` also attempted a sandbox-blocked Swagger UI download.
- 2026-08-07 codex: Added selection-based archive creation for ZIP and 7z with a format-aware
  dialog, ZIP compression levels, `Alt+F5` pack, `Alt+Shift+F5` move-to-archive, and `Alt+F6`
  extraction through the ordinary operation path. Creation validates destination/format agreement,
  refuses an existing destination, publishes transactionally, and only removes move sources after
  a readable archive has been created. The generated transport contract now carries archive format
  and compression options; documentation reflects the actual ZIP/7z/RAR capabilities.
- 2026-08-08 codex: Resumed and reviewed the completed implementation. Focused TDD coverage
  includes ZIP/7z creation, request validation, non-overwrite safety, move-after-readable-archive,
  dialog naming, and the `Alt+F5` / `Alt+Shift+F5` dispatches. `git diff --check` passes. Re-running
  the focused Rust/frontend suites was blocked by another concurrent build/test process holding
  the shared build directory and pnpm database; no feature defect was observed.
