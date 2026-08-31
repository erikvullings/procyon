# 0078 Workspace domain model refinement (spec §5.3)

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: backend
Depends on: 0006

## Context
`file-manager-coding-agent-spec.md` §5.3.2–§5.3.5 were fleshed out in much more detail after task
0006 shipped `fm-domain`'s workspace types. This task reconciles the two — 0006's own Agent Notes
flagged this exact possibility ("to be revisited if a later task's contract disagrees"). Two
concrete deviations matter beyond naming:

- §5.3.3 explicitly forbids one structure mixing persisted configuration with temporary UI state,
  but today's `DirectoryViewState` (in `fm-domain`) mixes the persisted `sort` field with
  frontend-session-only `selected_entry_ids`/`cursor_entry_id` inside the same type that gets
  serialized as part of a `Workspace` — meaning selection/cursor would currently be persisted to
  disk if a workspace were saved as-is.
- `fm-transport-dto`'s `WorkspaceLayoutDto::Pane` is already a named `{ pane_id }` struct variant
  (task 0007), but the underlying domain `WorkspaceLayout::Pane(PaneId)` is a tuple variant — 0007's
  own Agent Notes already called out this mismatch.

## Acceptance Criteria
- `Workspace` gains `schema_version: u32`, `created_at`/`updated_at: DateTime<Utc>`, `revision: u64`
  and `operation_centre: OperationCentrePreferences { visible: bool, height: u32 }` (§5.3.3,
  §5.3.15 example).
- `PaneState` gains `title: Option<String>` and a pane-level `default_view: DirectoryViewConfiguration`
  (§5.3.4).
- The persisted per-tab view configuration (`sort`, `columns`, `show_hidden`, `folders_first`,
  `quick_filter`) moves into its own `DirectoryViewConfiguration` type that is `Serialize`/
  `Deserialize` and contains **no** frontend-only fields; `selected_entry_ids` and `cursor_entry_id`
  are removed from anything that gets persisted as part of a workspace (§5.3.3) — they belong to
  the frontend-only `WorkspaceViewState`, which stays a TS-only concept (0082), not a Rust domain
  type.
- `ColumnConfiguration { column_id: String, width: u32, visible: bool }` backs
  `DirectoryViewConfiguration.columns` (§5.3.4, §5.3.15 example).
- `TabState` gains `title_override: Option<String>` and `pinned: bool` (§5.3.4).
- `NavigationHistory` gains an explicit `current: Location` field alongside `back`/`forward`
  matching §5.3.4's shape, or the deviation (keeping `TabState.location` as the sole current-location
  source of truth) is explicitly documented as a considered choice rather than an oversight.
- `WorkspaceLayout::Pane` becomes a struct variant with a named `pane_id: PaneId` field (not a tuple
  variant), and `SplitDirection` is renamed `SplitAxis` with `Horizontal`/`Vertical` variants,
  matching §5.3.5 — verified by a test that the JSON shape is byte-for-byte the §5.3.5/§5.3.15
  examples (`{"type":"pane","paneId":"..."}`).
- `fm-transport-dto`'s existing `From`/`Into` conversions (0007) still compile and round-trip after
  the rename — update them rather than duplicating logic.
- Unit tests: serde round-trip of the full refined `Workspace` against the literal §5.3.15 JSON
  example; a test proving `DirectoryViewConfiguration` cannot represent selection/cursor state.
- `cargo clippy --workspace --all-targets -- -D warnings` and `missing_docs` stay clean; every
  renamed/added field is documented.

## Implementation Notes
- This renames types shipped by task 0006 (already `done`) and consumed by 0007's DTOs — search
  the workspace for `Workspace`, `PaneState`, `TabState`, `WorkspaceLayout::Pane(`, `SplitDirection`
  before renaming (an IDE symbol rename keeps call sites in sync); update `fm-transport-dto`'s
  conversions in the same change.
- Do not add a Rust type for `WorkspaceViewState` — it is frontend-only per §5.3.3.
- Keep this task scoped to `fm-domain` (plus the minimal `fm-transport-dto` conversion fixups it
  forces); new DTO fields/endpoints for the command surface are task 0080's concern.

## Agent Notes
- 2026-07-29 claude: Two acceptance-criteria forks were left explicitly open to a choice; asked the
  user rather than guessing, per the acceptance criteria's own wording:
  - `SortKey { field: SortField }` (closed enum) → replaced with `SortDescriptor { column_id:
    String, direction }`, matching §5.3.15's `{"columnId":"core.name",...}` shape and putting
    built-in and plugin-provided columns on the same open-string footing as `ColumnConfiguration`.
    `SortField` is removed entirely (no remaining callers).
  - `NavigationHistory` keeps only `back`/`forward`; the user chose **not** to add an explicit
    `current: Location` field, so `TabState.location` stays the sole source of truth for the
    current location. Documented directly on `NavigationHistory` as a considered deviation from
    §5.3.4, not an oversight.
- `crates/fm-domain/src/workspace.rs`: `Workspace` gained `schema_version`, `created_at`,
  `updated_at`, `revision`, `operation_centre: OperationCentrePreferences { visible, height }`.
  `PaneState` gained `title: Option<String>` and `default_view: DirectoryViewConfiguration`.
  `DirectoryViewState` was replaced by `DirectoryViewConfiguration { sort, columns, show_hidden,
  folders_first, quick_filter }` — `#[serde(deny_unknown_fields)]` so it runtime-rejects
  `selectedEntryIds`/`cursorEntryId` in addition to not having Rust fields for them (verified by
  `directory_view_configuration_cannot_represent_selection_or_cursor_state`). Added
  `ColumnConfiguration { column_id, width, visible }` and a minimal `PersistedFilter { query:
  String }` (spec §24: plain-text quick filter only for now; glob/regex is a later feature, no
  `PersistedFilter` shape is spelled out elsewhere in the spec). `TabState` gained
  `title_override: Option<String>` and `pinned: bool`. `WorkspaceLayout::Pane` is now a struct
  variant `{ pane_id }`; `SplitDirection` is renamed `SplitAxis`, and the `Split` variant's field is
  renamed `direction` → `axis` to match §5.3.5's JSON key — required a per-variant
  `#[serde(rename_all = "camelCase")]` on `WorkspaceLayout`'s struct variants (the enum-level
  `rename_all` alone does not rename struct-variant fields in serde); a dedicated test asserts the
  serialized `Pane` JSON is byte-for-byte `{"type":"pane","paneId":"..."}`.
- Deliberately did **not** apply camelCase to the rest of `fm-domain`'s workspace types: every other
  `fm-domain` type (including `Location`) keeps Rust-native snake_case JSON, and only
  `fm-transport-dto` owns the camelCase wire format — matching the crate's existing, pre-existing
  convention. `WorkspaceLayout` is the one type the spec's own Rust snippet (§5.3.5) shows with
  `#[serde(tag = "type", rename_all = "camelCase")]` directly on the domain type, so that one stays
  camelCase-tagged in `fm-domain` too. Consequently the "round trip against the literal §5.3.15
  JSON" test lives in two places: `fm-domain::workspace::tests::workspace_round_trips_against_the_literal_spec_example_json`
  transcribes the same example's content into this crate's snake_case convention, while
  `fm-transport-dto::workspace::tests::workspace_dto_round_trips_against_the_literal_spec_example_json`
  deserializes the §5.3.15 JSON **verbatim** (byte-for-byte, including the `+02:00` offset
  timestamps) into `WorkspaceDto`, which is where the wire format is actually owned.
- `crates/fm-transport-dto/src/workspace.rs`: updated `WorkspaceDto`/`PaneStateDto`/`TabStateDto`
  and added `OperationCentrePreferencesDto`, `DirectoryViewConfigurationDto`,
  `ColumnConfigurationDto`, `SortDescriptorDto`, `PersistedFilterDto`, `SplitAxisDto` to mirror the
  domain changes field-for-field; existing `From`/`Into` conversions were updated in place (no
  duplicated logic). `SortFieldDto` removed (no replacement needed — DTOs never used it beyond
  `SortKeyDto`).
- Confirmed via `grep` before renaming that only `fm-domain` and `fm-transport-dto` reference these
  types; `fm-events`' `WorkspacePayload`/`PaneStatePayload`/etc. are intentionally independent
  mirrors (its own module doc: "mirror the OpenAPI-facing DTOs rather than depending on
  `fm-transport-dto`") and do not import from either crate, so they were left untouched — they now
  represent an older, unrefined shape (no `title`/`pinned`/`operationCentre`/etc., still tuple-style
  `SortField`). This is a known follow-up, likely for whichever task next touches
  `workspace.*Changed` events, not a silent gap in this task's own scope.
  `frontend/src/models/workspace.ts` is a hand-written TS mirror in the same position — also
  untouched, out of scope per the task's own note (0080/0082 own the command surface and frontend
  projection).
  `frontend/openapi/openapi.json`/the Orval client do not yet reference `WorkspaceDto` in any
  endpoint (workspace REST endpoints are task 0080's concern), so `pnpm run api:check` has nothing
  to regenerate from this change.
- Verified: `cargo test -p fm-domain` — 29 tests, all passing (was 25 before this task; net +4 new
  tests: the byte-for-byte `Pane` shape test, the `deny_unknown_fields` selection/cursor-rejection
  test, the literal-example round trip, plus the pre-existing tests updated for the renamed types).
  `cargo test -p fm-transport-dto` — 30 tests, all passing (+1 new: the verbatim §5.3.15 round trip
  through `WorkspaceDto`). `cargo test --workspace` — 41 passing test binaries/suites, 0 failed,
  confirming `fm-events`, `fm-application` and every other crate still compile and pass unaffected.
  `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo doc -p fm-domain -p fm-transport-dto --no-deps` (proxy for the `missing_docs` lint) are all
  clean.
- Known gap: none against this task's own (post-clarification) acceptance criteria. The
  `fm-events`/frontend-model divergence noted above is an explicitly out-of-scope follow-up, not a
  gap in this task.

