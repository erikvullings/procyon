# 0166 Native indexed search

Status: done
Priority: medium
Subsystem: backend, search, platform
Depends on: 0058, 0068, 0089

## Context

Recursive search is provider-neutral and correct but must walk the filesystem. Local searches can be
nearly instantaneous by querying Spotlight on macOS and Windows Search where available, while
retaining recursive traversal as the portable fallback.

## Acceptance Criteria

- A search-acceleration interface reports supported predicates and scopes independently of the VFS
  provider contract.
- macOS Spotlight and Windows Search adapters return normalized entry references without exposing
  platform query syntax to the frontend.
- Search planning uses an index only when it can preserve requested semantics; otherwise it clearly
  falls back to existing recursive filename/content search.
- Results outside the requested root, stale index entries, inaccessible paths, and aliases are
  filtered or identified safely.
- Users can see whether a result set is indexed, live-recursive, or mixed, and can request a refresh.
- Query cancellation, paging, and HTTP/Tauri parity match the existing search lifecycle.
- Tests cover query translation, unsupported predicates, scope enforcement, stale/missing results,
  fallback behavior, ordering, and cancellation.

## Implementation Notes

- Keep native indexing an optimization. 0068 and 0089 remain the behavioral reference.
- Do not shell-interpolate user queries. Use platform APIs or strictly argument-separated processes
  behind narrow adapters.
- Linux indexing integrations may be added later; lack of an index must not reduce functionality.

## Agent Notes

- 2026-08-28: Created from the product feature review. Correct fallback semantics are more important
  than forcing every query through a native index.
- 2026-08-28 copilot: Completed native indexed search.
  - Added layer-1 `fm-search-acceleration`: a VFS-independent, cancellation-aware local-index
    contract with explicit literal-name predicate and recursive-directory scope capabilities,
    normalized absolute path references, alias identification, and unsupported/unavailable errors.
  - Added argument-separated Spotlight (`mdfind -onlyin`) and Windows Search
    (`Search.CollatorDSO` through a fixed PowerShell script with values as argv) adapters. The
    search engine injects the accelerator independently of the platform/VFS contracts; both server
    and Tauri hosts select their native adapter, while all defaults and tests retain the unsupported
    recursive fallback.
  - The planner only indexes recursive literal filename searches whose advertised capabilities
    preserve the requested semantics. Content, glob, non-recursive, git-status, and symlink
    requests use the existing live traversal. Indexed paths are canonically constrained to their
    root and discard stale, inaccessible, symlink, Finder-alias, hidden, and `.git` candidates.
    Native-index failure falls back to recursion.
  - Start responses and `search.resultsBatch` events expose `indexed`, `liveRecursive`, or `mixed`.
    Search tabs render the mode and have a refresh control that reruns the persisted request through
    the ordinary lifecycle; early batch modes are retained until the start response resolves.
  - Verified: 3 new contract tests; 8 indexed-engine behavior tests (translation/scope/fallback,
    stale/outside/alias filtering, paging/order, mixed mode, and cancellation); 4 Spotlight adapter
    tests; Windows adapter cross-checked with `cargo check --target x86_64-pc-windows-msvc`;
    Tauri host checked; HTTP search route tests; frontend typecheck and 189 targeted frontend tests.
    Affected Rust package suites and architecture fitness test pass; OpenAPI/Orval regenerated; full
    `pnpm run lint` passes (with three pre-existing Biome CSS warnings). Full `fm-server` testing
    still has two unrelated disk-usage expectation failures in `files_routes` (HTTP 202 vs expected
    200/409); native-search route tests pass.
