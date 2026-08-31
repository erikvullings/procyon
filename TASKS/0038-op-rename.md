# 0038 Operation: rename

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0037

## Context
`file-manager-coding-agent-spec.md` §33 step 7 item 2, §17 safety requirements, §27 (case-only
renames are a named integration test).

## Acceptance Criteria
- `OperationKind::Rename` implemented via the provider's `rename`.
- Case-only renames work on case-insensitive filesystems (macOS/Windows) — typically via a
  two-step rename through a temporary name; covered by an integration test.
- Refuses to overwrite an existing entry unless the conflict policy explicitly says so; never
  silently replaces a file or a directory (§17, §35).
- `F2` triggers rename; inline rename in the directory table with the basename pre-selected
  (extension excluded from the initial selection), `Esc` cancels, `Enter` commits.
- Invalid names are rejected before the request is sent, with an inline message.
- The renamed entry keeps the cursor and selection after the delta arrives.
- Integration tests: plain rename, case-only rename, rename onto an existing name (rejected),
  Unicode rename, rename of a directory with open children, permission denied where testable.

## Implementation Notes
- Rename within a directory is a metadata operation, not a copy; never fall back to copy+delete
  here — cross-directory moves are 0041.
- Inline rename lives in the directory table but the mutation goes through the operation engine
  (§35).

## Agent Notes
- 2026-07-31 codex: Implemented provider-backed rename jobs without copy/delete fallback, including
  collision-safe behavior, stable entry identity, Unicode and non-empty-directory renames, and a
  two-step temporary-name path for case-only renames on macOS/Windows. Native permission failures
  map to the existing typed denial; permission enforcement remains platform/user conditional.
- 2026-07-31 codex: Added inline F2 rename in the virtualized directory table with basename-only
  initial selection, pre-request name validation and inline feedback, Esc cancellation, and Enter
  commit through the semantic operation client. Stable entry IDs preserve cursor and selection
  through delta replacement.
- 2026-07-31 codex: Added 3 provider tests, 2 application integration tests, and 1 frontend rename
  test. Verified these directly plus the full affected package suites, strict frontend typecheck,
  full `pnpm test`, `pnpm run lint`, API freshness, workspace formatting, and Clippy. Axum and Tauri
  share the existing operation transport path; no packaged Tauri GUI smoke test was performed.
