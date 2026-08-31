# 0018 Local filesystem provider: listing, paging and metadata

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0017

## Context
`file-manager-coding-agent-spec.md` §6 ("initial provider") and §33 step 4. The local provider is
the only provider in milestone 1 and must be correct about the awkward cases before the UI is built
on top of it.

## Acceptance Criteria
- `fm-vfs-local` implements `FileSystemProvider` for `list`, `metadata`, and declares its real
  `ProviderCapabilities`; mutating methods may return `Unsupported` until 0037–0044.
- Listing produces `EntrySummary` values without expensive metadata (no checksums, no MIME sniffing,
  no image dimensions) — those come from the separate metadata call (§5.2).
- Correct handling of: files, directories, hidden entries (dotfiles and the Windows hidden
  attribute), symbolic links, Windows reparse points and junctions, unreadable directories, Unicode
  names, very long paths, empty files, sparse files where feasible, and names containing spaces and
  shell-sensitive characters.
- Symbolic links are **not** followed by default and are flagged in the entry so the UI can show an
  indicator (§6, §35).
- Results are paged with a `continuation_token`; listing a 100,000-entry directory returns the first
  page promptly and never buffers the whole directory before returning.
- All listing work happens on the blocking pool / async I/O; a `CancellationToken` aborts within one
  page.
- Integration tests use temporary directories only (§27) and cover every bullet above; symlink and
  junction tests are `cfg`-gated per platform and report explicitly when unsupported.

## Implementation Notes
- macOS aliases are a later enhancement (§6) — leave a documented gap, not a silent one.
- Use `tokio::fs` / `std::fs` on `spawn_blocking`; never block the async executor.
- Recursive traversal helpers must be cycle-protected (device+inode set on Unix, file id on
  Windows) — needed by 0040 and 0044.

## Agent Notes
- 2026-07-30 codex: Implemented `LocalFileSystemProvider` with truthful `LIST` capabilities,
  cancellable async directory listing, bounded offset-token paging, lightweight `EntrySummary`
  construction, separate detailed permission metadata, typed filesystem errors, and explicit
  unsupported results for read/write/mutation/watch operations. Symlinks are inspected without
  following them; Windows reparse attributes (including junctions) are treated as links.
- 2026-07-30 codex: Added nine temporary-directory integration tests covering files, directories,
  dotfiles, Unicode and shell-sensitive names, empty and sparse files, long paths, typed missing
  and not-directory failures, unreadable Unix directories where permissions are enforced,
  cancellation, metadata/capabilities, symlinks, multi-page completeness, and a 100,000-entry
  directory whose first 64-entry page is returned without a known-total scan. Windows hidden and
  reparse behavior has a `cfg(windows)` test that reports explicitly when link creation privileges
  are unavailable.
- 2026-07-30 codex: Verified the nine task-specific tests with `cargo test -p fm-vfs-local`,
  `cargo check -p fm-vfs-local --all-targets`, repository-wide Rust formatting and strict Clippy,
  and the full `pnpm test` suite. `pnpm run lint` passes all Rust checks but remains non-zero on
  pre-existing, task-unrelated Biome findings in `frontend/vite.config.ts`,
  `scripts/architecture-docs.test.mjs`, and `scripts/ci-workflow.test.mjs`; none was changed.
  `CLAUDE.md` is absent, so there was no scoped file to update.
- 2026-07-30 codex: Known platform gaps: macOS Finder aliases remain ordinary files as explicitly
  deferred by the task; Windows-specific behavior is CI-gated but was not executable on this macOS
  host. No recursive traversal helper is introduced by listing or metadata, so cycle protection
  remains required when tasks 0040/0044 add recursive traversal.
