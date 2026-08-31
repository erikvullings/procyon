# 0007 Transport DTOs and OpenAPI schemas

Status: done
Priority: high
Owner: unassigned
Agent: claude
Area: backend
Depends on: 0006

## Context
`file-manager-coding-agent-spec.md` §8 and §9 require versioned JSON DTOs documented with `utoipa`.
Rule 5 of §3 forbids reusing transport DTOs indiscriminately as internal domain models, so DTOs
live in `fm-transport-dto` with explicit conversions from/to `fm-domain`.

## Acceptance Criteria
- `fm-transport-dto` defines DTOs for the milestone-1 surface: `RuntimeCapabilitiesDto`,
  `WorkspaceDto`, `PaneStateDto`, `TabStateDto`, `LocationDto`, `EntrySummaryDto`,
  `DirectorySnapshotDto`, `ListDirectoryRequest`, `NavigateRequest`, `EntryMetadataRequest`,
  `EntryMetadataDto`, and the error DTO from §8.
- All DTOs derive `Serialize`, `Deserialize`, `ToSchema`, use `#[serde(rename_all = "camelCase")]`
  and RFC 3339 timestamps.
- Tagged unions use string discriminators (`#[serde(tag = "type", rename_all = "camelCase")]`).
- `ApplicationErrorDto { code, message, requestId, details }` matches the example in §8; codes are a
  closed enum with stable camelCase names and never leak raw OS error strings.
- `From`/`TryFrom` conversions between domain types and DTOs, with unit tests round-tripping each
  DTO through JSON and asserting the exact camelCase field names.
- Important schemas carry `#[schema(example = ...)]` values (§9).

## Implementation Notes
- Reserve naming for endpoints not yet implemented (operations, actions, plugins, settings) but do
  not add DTOs without a consumer (§35 — no speculative abstractions).
- `fm-transport-dto` may depend on `fm-domain` and `utoipa`, never the reverse.

## Agent Notes
- 2026-07-29 claude: Implemented `fm-transport-dto` as seven modules mirroring
  `fm-domain`'s module split: `location.rs` (`LocationDto`), `entry.rs`
  (`EntryKindDto`, `EntrySummaryDto`, `PermissionsInfoDto`, `OwnershipInfoDto`,
  `ImageDimensionsDto`, `MediaMetadataDto`, `ArchiveInfoDto`,
  `EntryMetadataDto`), `snapshot.rs` (`LoadingStateDto`,
  `DirectorySnapshotDto`), `workspace.rs` (`SortFieldDto`, `SortDirectionDto`,
  `SortKeyDto`, `NavigationHistoryDto`, `DirectoryViewStateDto`,
  `TabStateDto`, `PaneStateDto`, `SplitDirectionDto`, `WorkspaceLayoutDto`,
  `WorkspaceDto`), `requests.rs` (`ListDirectoryRequest`, `NavigateRequest`,
  `EntryMetadataRequest`), `runtime.rs` (`RuntimeKindDto`, `PlatformKindDto`,
  `RuntimeCapabilitiesDto`, matching the §21 TS interface field-for-field) and
  `error.rs` (`ApplicationErrorCode`, `ApplicationErrorDto`).
- Every DTO that has a domain counterpart (all except the request DTOs,
  `RuntimeCapabilitiesDto` and `ApplicationErrorDto`, none of which have one
  yet) has explicit, infallible `From<Domain> for Dto` and `From<Dto> for
  Domain` conversions, unit-tested by round-tripping domain → DTO → domain and
  asserting equality with the original domain value.
- ID fields are represented as bare `uuid::Uuid` (UUID-backed domain ids) or
  `String` (`ProviderId`) rather than reusing the `fm-domain` newtypes
  directly, since `fm-domain` cannot depend on `utoipa` to implement
  `ToSchema` for them and the orphan rule blocks implementing it from
  `fm-transport-dto`. This required one small, backwards-compatible addition
  to `fm-domain/src/ids.rs`: `From<Uuid> for $name` / `From<$name> for Uuid`
  on the `uuid_id!` macro (covering `WorkspaceId`, `PaneId`, `TabId`,
  `EntryId`, `OperationId`), tested by a new
  `uuid_id_round_trips_through_the_uuid_conversion_traits` unit test. No other
  change was made to already-`done` task 0006's types.
- Tagged unions (`LoadingStateDto`, `WorkspaceLayoutDto`) use
  `#[serde(tag = "type", rename_all = "camelCase")]` per the acceptance
  criteria; `WorkspaceLayoutDto::Pane` uses a named `pane_id` field (rather
  than mirroring the domain's tuple variant) so the tagged-union shape stays
  uniform across variants.
- `ApplicationErrorCode` is a closed enum covering the milestone-1 surface
  (`notFound`, `permissionDenied`, `invalidRequest`,
  `destinationAlreadyExists`, `providerUnavailable`, `operationCancelled`,
  `internal`); it has no domain-side conversion because no `ApplicationError`
  type exists yet (it belongs to `fm-application`, a later task). `details` is
  `Option<serde_json::Value>` to match the §8 example's free-form object.
  `RuntimeCapabilitiesDto` likewise has no domain conversion — capabilities are
  runtime-detected, not domain state.
- Request DTOs (`ListDirectoryRequest`, `NavigateRequest`,
  `EntryMetadataRequest`) each carry a client-generated `request_id: Uuid` so
  a superseded request's late response can be recognised via the matching
  `DirectorySnapshotDto.request_id`, consistent with `fm-domain`'s existing
  `DirectorySnapshot::request_id` doc comment. `ListDirectoryRequest` also
  carries `pane_id` and `continuation_token`; `EntryMetadataRequest` carries
  `entry_id` and `location` so the request can be dispatched to the owning
  provider without a prior lookup.
- Every DTO derives `Serialize`, `Deserialize`, `ToSchema`, uses
  `#[serde(rename_all = "camelCase")]`; timestamps reuse
  `chrono::DateTime<Utc>` directly (RFC 3339 via chrono's serde impl, and
  `ToSchema` via `utoipa`'s `chrono` feature). `Uuid`, `BTreeMap<K, V>` and
  `serde_json::Value` schema support all come from `utoipa`'s built-in impls
  (confirmed by reading `utoipa-5.5.0`'s source) — no `value_type` overrides
  were needed.
- `#[schema(example = ...)]` examples were added to the "important" schemas
  per §9: `LocationDto`, `EntrySummaryDto`, `DirectorySnapshotDto`,
  `WorkspaceDto`, `RuntimeCapabilitiesDto`, and all three request DTOs, plus
  `ApplicationErrorDto` (reproducing the exact §8 example). Discovered that
  `utoipa`'s derive macro parses the `json!(...)` expression itself and
  re-emits it with fully qualified paths, so files using the attribute do not
  need a local `use serde_json::json;` — confirmed by a clean `cargo build`
  with the import removed.
- No DTO was added without a consumer per the Implementation Notes:
  `DirectoryDelta` (domain) and operations/actions/plugins/settings DTOs were
  deliberately left out of scope, since acceptance criteria doesn't name them
  and no endpoint consumes them yet.
- Verified: 27 new tests in `fm-transport-dto` (`cargo test -p
  fm-transport-dto`), 1 new test in `fm-domain` for the `Uuid` conversions
  (`cargo test -p fm-domain`, 26/26 passing, up from 25). Full workspace:
  `cargo test --workspace` all green (fm-test-support's 8 unit tests plus the
  1 `workspace_architecture` layering fitness test unaffected — `fm-domain` at
  layer 0 and `fm-transport-dto` at layer 1 depending only on it is already
  the layering the fitness test expects). `cargo fmt --all --check` clean.
  `cargo clippy --workspace --all-targets -- -D warnings` clean. `cargo build
  --workspace` clean.
- Known gap: none against the stated acceptance criteria. `RuntimeCapabilitiesDto`
  and `ApplicationErrorDto` intentionally have no `From`/`TryFrom` domain
  conversions since no corresponding domain/application type exists yet —
  this is expected to be picked up when `fm-application`'s error type and the
  runtime-detection service are built.

