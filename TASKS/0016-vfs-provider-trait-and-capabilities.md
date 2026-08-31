# 0016 VFS provider trait, capabilities and errors

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0006

## Context
`file-manager-coding-agent-spec.md` §6 defines the provider abstraction that keeps the engine
independent of the local filesystem, so archives, SFTP, S3 and search results can be added later
without redesigning the core.

## Acceptance Criteria
- `fm-vfs` defines `FileSystemProvider` (async trait) with the methods from §6: `id`,
  `capabilities`, `list`, `metadata`, `create_directory`, `rename`, `remove`, `open_read`,
  `open_write`, `watch`.
- `ProviderCapabilities` bitflags exactly as listed in §6.
- Supporting types: `EntryRef`, `ListOptions`, `DirectoryPage`, `RemoveOptions`, `WriteOptions`,
  `ProviderReadStream`, `ProviderWriteStream`, `ProviderChangeStream`.
- `VfsError` is a `thiserror` enum with variants covering not-found, permission-denied,
  already-exists, not-a-directory, is-a-directory, unsupported-capability, cancelled, io,
  invalid-location — each mapping to a stable machine-readable code (§8).
- Every long-running method accepts a `CancellationToken` (§35).
- A `ProviderRegistry` resolves a `Location` to its provider and returns a typed error for unknown
  provider ids.
- Unit tests: capability checks reject unsupported operations before any I/O; registry resolution.
- `fm-vfs` does not depend on Axum, Tauri or `fm-application`.

## Implementation Notes
- Design for archive/SFTP/WebDAV/S3/SMB/search/trash/recent providers but implement none of them
  (§6 "future providers", §35 no speculative abstractions beyond planned features).
- Read/write streams should be `AsyncRead`/`AsyncWrite` based so copies can stream without buffering
  whole files.

## Agent Notes
- 2026-07-30 codex: Implemented the documented `fm-vfs` boundary as focused modules for the async
  `FileSystemProvider` trait, all fourteen §6 `ProviderCapabilities` flags, provider-neutral request
  and page types, boxed `AsyncRead`/`AsyncWrite` streams, a `DirectoryDelta` change stream, typed
  `VfsError` values with stable camel-case codes, and `ProviderRegistry` dispatch by
  `Location.provider_id`. Every async provider operation accepts a `CancellationToken`; safe
  defaults prevent recursive/trash removal and overwrite unless explicitly requested.
- 2026-07-30 codex: Followed TDD through the public crate API: first added the integration contract
  tests and confirmed they failed with unresolved `fm-vfs` imports, then implemented the minimum
  contract and refactored it into capability, error, provider, registry and supporting-type
  modules. Five task-specific tests cover the exact capability bits, pre-I/O rejection of an
  unsupported capability, registry success/unknown-provider failure, and every required stable
  error code.
- 2026-07-30 codex: Verified `cargo test -p fm-vfs` (5 task-specific tests), `cargo test
  --workspace`, and `pnpm test` (full Rust, 70 frontend and 28 script tests) all pass.
  `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.
  `pnpm run lint` reaches and passes the Rust checks but remains non-zero on pre-existing,
  task-unrelated Biome findings in `frontend/vite.config.ts`,
  `scripts/architecture-docs.test.mjs`, and `scripts/ci-workflow.test.mjs`; none was changed.
  `CLAUDE.md` is absent from this repository, so there was no scoped file to update.
- 2026-07-30 codex: Confirmed `fm-vfs` depends only on `fm-domain` and general-purpose async/error
  crates; the workspace architecture fitness test passes and the crate has no Axum, Tauri or
  `fm-application` dependency. Known gap: none within task 0016; concrete providers remain owned by
  their planned tasks.
