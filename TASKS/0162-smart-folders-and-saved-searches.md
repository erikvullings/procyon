# 0162 Smart folders and saved searches

Status: done
Priority: high
Subsystem: backend, frontend, search
Depends on: 0030, 0068, 0089

## Context

Recursive filename and content search are currently ad hoc actions. Users should be able to preserve
useful queries as virtual locations such as "modified this week", "large videos", "uncommitted
files", or "documents containing a phrase".

## Acceptance Criteria

- A typed query model supports useful combinations of name, type/MIME, size, modified time, content,
  git status, tags/metadata, and provider/location scope.
- Users can save, rename, edit, pin, and delete named searches and open them like other locations in
  either pane or a tab.
- Results use stable entry references, support paging/cancellation, and expose which predicates could
  not be evaluated by a provider.
- Opening an entry or its parent from a result preserves normal pane navigation and selection
  behavior.
- Saved searches persist without credentials or transient connection tokens and migrate cleanly when
  the settings shape changes.
- Search execution remains bounded and provider-neutral; unsupported expensive predicates require
  explicit user confirmation or a clear limitation.
- Tests cover query serialization, predicate combinations, provider capability gaps, cancellation,
  stale results, persistence, and virtual-location navigation.

## Implementation Notes

- Extend the existing search virtual-location model instead of introducing a separate smart-folder
  result store.
- Keep the query representation structured and versioned. Do not persist an opaque backend command or
  executable expression.
- 0166 may later accelerate supported scopes, but this task must remain correct with the existing
  recursive search implementation.

## Agent Notes

- 2026-08-28: Created from the product feature review. This is intended to turn existing search
  capabilities into a reusable navigation primitive.
- 2026-08-28: Implemented versioned structured queries, persisted saved-search CRUD and pinning,
  pane/tab opening, stable result references, provider limitation reporting, and settings
  sanitization/migration. Verified with repository-wide lint and tests: 1,346 Rust tests, 1,506
  frontend tests, all Rust doctests, and 40 script tests passed.
