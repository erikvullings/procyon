# 0006 Core domain model in fm-domain

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: backend
Depends on: 0001

## Context
Implement the core domain model from `file-manager-coding-agent-spec.md` §5. These types are the
foundation for every later crate and must not depend on Axum, Tauri or transport DTOs.

## Acceptance Criteria
- Strongly typed newtype identifiers with `Display`, `FromStr`, `Serialize`/`Deserialize`:
  `WorkspaceId`, `PaneId`, `TabId`, `EntryId`, `OperationId`, `ProviderId`, `PluginId`, `ActionId`.
- `Location { provider_id, uri }` is serializable, round-trips through string form, and preserves
  platform-specific paths (§5.1). Parsing itself is task 0017; this task defines the type and its
  invariants.
- `EntryKind`, `EntrySummary` exactly as in §5.2, with `Option` fields for metadata that may be
  unavailable.
- `Workspace`, `PaneState`, `TabState`, `NavigationHistory`, `DirectoryViewState`,
  `WorkspaceLayout` per §5.3 — with no hard-coded assumption of exactly two panes.
- `DirectorySnapshot`, `DirectoryDelta`, `LoadingState` per §5.4.
- `EntryMetadata` for the detailed (non-eager) metadata described in §5.2.
- Unit tests for id round-tripping, `Location` serialization and snapshot/delta serde stability.
- Crate has no dependency on `axum`, `tauri`, `reqwest` or `utoipa`.

## Implementation Notes
- Timestamps are `chrono::DateTime<Utc>`; serialization uses RFC 3339 (§8).
- `metadata_revision` and snapshot `revision` are monotonic `u64` used for stale-response rejection.
- Document every public item (§35).

## Agent Notes
- 2026-07-29 claude: TDD per module. For each of `ids.rs`, `location.rs`, `entry.rs`,
  `workspace.rs` and `snapshot.rs`, wrote the `#[cfg(test)]` module first (referencing types that
  did not exist yet), confirmed `cargo test -p fm-domain` failed to compile (red — 5 `E0432`
  unresolved-import errors), then implemented each type to make it compile and pass (green).
- 2026-07-29 claude: Identifiers (`ids.rs`) — two `macro_rules!` (`uuid_id!`, `string_id!`) generate
  `WorkspaceId`, `PaneId`, `TabId`, `EntryId`, `OperationId` (UUID-backed, `Copy`, random `new()`,
  fallible `FromStr` via a `thiserror` `IdParseError`) and `ProviderId`, `PluginId`, `ActionId`
  (`String`-backed, infallible `FromStr`, e.g. `ProviderId::new("file")`). All eight have `Display`,
  `FromStr`, `Serialize`/`Deserialize` per the acceptance criteria.
- 2026-07-29 claude: `Location { provider_id, uri }` (`location.rs`) exactly as spec §5.1, with a
  doc comment recording the invariant that `uri` already carries the full scheme-inclusive text
  (e.g. `file:///C:/Users/Erik/Documents`) while `provider_id` duplicates the scheme so callers
  never need to re-parse `uri` to dispatch to a provider. Parsing/normalization is explicitly left
  to task 0017.
- 2026-07-29 claude: `entry.rs` — `EntryKind` has three variants (`File`, `Directory`, `Symlink`);
  `Symlink` is justified directly by the directory-table spec (§15 "symlink/junction indicators").
  `EntrySummary` matches spec §5.2 field-for-field. `EntryMetadata` covers all eight categories
  listed in §5.2 (permissions, ownership, extended attributes, checksums, image dimensions, media
  metadata, archive information, plugin-provided fields) via small sub-structs
  (`PermissionsInfo`, `OwnershipInfo`, `ImageDimensions`, `MediaMetadata`, `ArchiveInfo`) plus
  `BTreeMap`s for the open-ended categories (extended attributes, checksums keyed by algorithm name
  per task 0077, and `serde_json::Value`-typed plugin fields) — deliberately not exhaustive, since
  the exact shape of each category is decided by later tasks (metadata/plugin crates).
- 2026-07-29 claude: `workspace.rs` — `Workspace`/`PaneState`/`TabState`/`NavigationHistory` match
  spec §5.3. `WorkspaceLayout` is a recursive binary `Split { direction, ratio, first, second }` /
  `Pane(PaneId)` tree rather than a flat two-element list: a real UI splitter is always pairwise, so
  three-or-more panes nest further splits — this directly satisfies the acceptance criterion "no
  hard-coded assumption of exactly two panes" without speculative N-ary ratio semantics. Task 0026
  will drive the two-pane UI from a two-node instance of this tree; task 0027 depends on
  `NavigationHistory`. `DirectoryViewState.sort: Vec<SortKey>` starts with one element so task
  0029's "multiple sort keys later" needs no data-model rewrite, matching that task's own note.
- 2026-07-29 claude: `snapshot.rs` — `DirectorySnapshot`/`DirectoryDelta`/`LoadingState` match spec
  §5.4 exactly (including the `Reset` variant used when an incremental delta isn't possible).
  `LoadingState::Error { message }` deliberately carries only a user-readable message, not a raw OS
  error, per the §8 API rule "never expose raw OS errors directly to the frontend"; structured
  `code`/`requestId`/`details` fields belong to the transport DTO (task 0007), not this domain type.
- 2026-07-29 claude: Crate deps (`Cargo.toml`) are exactly `chrono`, `serde`, `serde_json`,
  `thiserror`, `uuid` (all `workspace = true`) — no `axum`, `tauri`, `reqwest` or `utoipa`, confirmed
  both by inspection and by the existing `fm-test-support` architecture fitness test (layer 0, no
  workspace-internal dependencies).
- 2026-07-29 claude: Verified — `cargo test -p fm-domain`: 25 new unit tests, all passing (id
  round-tripping via `Display`/`FromStr`/serde for all 8 id types; `Location` serde round-trip
  including a Windows-style path; `EntrySummary`/`EntryKind`/`EntryMetadata` serde round-trips with
  metadata fields both absent and fully populated; `Workspace`/`WorkspaceLayout` (including a
  3-pane nested-split case)/`NavigationHistory`/`DirectoryViewState` serde round-trips;
  `DirectorySnapshot`/`DirectoryDelta`/`LoadingState` serde round-trips for every variant).
  `cargo test --workspace`: every other crate unaffected (fm-test-support still 8 unit + 1
  integration test, including the layering fitness test). `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo build --workspace` all clean —
  `missing_docs` (workspace lint, warn-by-default) passes, meaning every public item, including enum
  variants and struct fields, has a doc comment.
- 2026-07-29 claude: Known gap — none. Every acceptance-criteria bullet is met; `EntryKind`,
  `EntryMetadata`'s sub-structs, `WorkspaceLayout` and `DirectoryViewState` are not literally
  spelled out field-by-field in the spec text, so their exact shape is a documented design decision
  above rather than a spec transcription, to be revisited if a later task's contract disagrees.

