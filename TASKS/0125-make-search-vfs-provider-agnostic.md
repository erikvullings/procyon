# 0125 Make Search Engine VFS-provider agnostic

Status: done
Priority: medium
Subsystem: backend
Depends on: none

## Context

`SearchEngine` (`crates/fm-search/src/engine.rs`, ~1130 lines) bypasses the VFS abstraction entirely. It calls `root.to_native_path()` on every search root and then uses `ignore::WalkBuilder` directly on native paths. This hardcodes local-filesystem-only search. Searching over SFTP, archive, or other VFS providers would require significant rework of the engine.

The VFS `FileSystemProvider` trait already provides `list()`, `open_read()`, and `inspect()` — the hooks needed for provider-agnostic traversal. The engine should use these instead of `WalkBuilder`.

## Acceptance Criteria
- `SearchEngine::start()` uses `FileSystemProvider::list()` for directory traversal instead of `ignore::WalkBuilder`
- Content search uses `FileSystemProvider::open_read()` instead of direct `std::fs::File`
- Local filesystem search works identically to current behavior (same speed, same results)
- Search over SFTP locations works without code changes to the engine (just provider swapping)
- `ignore::WalkBuilder` dependency retained for local-only optimization (follow_links, hidden-file filtering) — provider-agnostic path is a superset, not a replacement
- All existing search tests pass

## Implementation Notes
- The `ignore` crate provides superior `.gitignore`-aware traversal. The VFS path won't have that unless the local provider exposes it. Consider: keep `WalkBuilder` as an optimization for `file://` roots (the engine already checks `root.to_native_path()` — if it succeeds, use `WalkBuilder`; otherwise fall through to VFS traversal)
- The provider-agnostic traversal path needs: recursive `list()` calls, hidden-file filtering, cancellation checks per-directory
- Content search already uses `ProviderReadStream` — verify this isn't already VFS-agnostic
- The `SearchFileSystemProvider` (presents results as `search://` virtual directory) is fine; it's the engine that needs work

## Agent Notes
