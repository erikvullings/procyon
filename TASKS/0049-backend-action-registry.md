# 0049 Backend action registry

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0036

## Context
`file-manager-coding-agent-spec.md` §18 — everything invokable from the UI is an action, and menus,
context menus, toolbars, the command palette and keyboard shortcuts all invoke the same registry.

## Acceptance Criteria
- `ActionDescriptor` as in §18: `id`, `title`, `description`, `category`, `default_shortcuts`,
  `context_requirements`, `parameter_schema`, `source`.
- Core actions registered with the ids listed in §18: `core.open`, `core.openWith`, `core.copy`,
  `core.move`, `core.rename`, `core.delete`, `core.createDirectory`, `core.openTerminal`,
  `core.copyPath`, `core.copyRelativePath` (plus selection/navigation actions from 0028).
  Actions whose feature does not exist yet are registered as unavailable, not omitted.
- `GET /api/v1/actions` → `listActions` and `POST /api/v1/actions/{actionId}/invoke` →
  `invokeAction`, mirrored as Tauri commands.
- Invocation carries a typed context (active pane, selection, cursor entry) and the backend
  re-validates `context_requirements` — the backend is authoritative even though the frontend may
  pre-evaluate availability for rendering (§18).
- Invoking an unavailable or unknown action returns a typed error, never a panic.
- Actions that mutate files delegate to the operation engine and return an `OperationId`.
- Unit tests: registration, duplicate-id rejection, context requirement evaluation, invocation
  routing.

## Implementation Notes
- `KeyChord` needs a serializable, platform-aware representation (`Cmd` vs `Ctrl`) shared with the
  frontend dispatcher (0050).
- The registry must accept plugin-contributed actions later (0053) — keep `ActionSource` open.

## Agent Notes
- Implemented bottom-up through the crate dependency graph, each layer with its own colocated
  tests (TDD), verified passing at each step before moving on.
  - `fm-domain::action` — plain domain types (`KeyChord`, `ActionSource`, `ActionDescriptor`,
    `ActionContextRequirements` with `none()`/`unimplemented()`/`selection()`/
    `single_selection()` constructors and `is_satisfied_by`, `ActionInvocationContext`). 7 tests.
  - `fm-application::action::ActionRegistry` — registration/lookup/availability
    (`with_core_actions()`, `register`, `get`, `list`, `require_available`). 5 tests.
  - `fm-application::error` — added `ActionNotFound`/`ActionUnavailable` variants with
    `.code()`/`.into_dto()` mappings; renamed the pre-existing `UnknownAction` variant to
    `ActionNotFound` for naming consistency. 2 new tests.
  - `fm-transport-dto::error` — added matching `ApplicationErrorCode::ActionNotFound` /
    `ActionUnavailable` wire codes (`"actionNotFound"`/`"actionUnavailable"`). 1 new test.
  - `fm-transport-dto::action` — wire DTOs (`KeyChordDto`, `ActionSourceDto`,
    `ActionContextRequirementsDto`, `ActionDescriptorDto`, `ActionInvocationContextDto`,
    `InvokeActionRequestDto`, `ActionResultDto`) with `From`/`Into` conversions. 8 tests (incl. a
    regression test added after catching a utoipa `ToSchema` bug, see below).
  - `fm-application::service::FileManagerService` — added an `actions: ActionRegistry` field,
    `list_actions()`, `invoke_action(action_id, request, idempotency_key)` (validates
    availability against the invocation context, then either returns immediately for
    non-mutating actions or deserializes `parameters` as a `StartOperationRequestDto` and
    delegates to `start_operation` for mutating ones), and a `mutating_operation_kind` helper.
    7 tests.
  - `apps/fm-server` — `GET /api/v1/actions` and `POST /api/v1/actions/{actionId}/invoke` Axum
    routes (`routes/action.rs`), `ActionNotFound → 404` / `ActionUnavailable → 409` status
    mapping in `error.rs`. 5 integration tests (`tests/action_routes.rs`).
  - `apps/fm-desktop/src-tauri` — mirrored `list_actions`/`invoke_action` Tauri commands in
    `commands.rs`, registered in both `generate_handler!` invocations in `lib.rs` (the real one
    and the test harness's mock builder).
  - Regenerated `frontend/openapi/openapi.json` and the Orval client
    (`frontend/src/api/generated/**`), and updated hand-written frontend models
    (`models/action.ts`, `models/requests.ts`) plus wired both `HttpFileManagerClient` and
    `TauriFileManagerClient`'s `listActions`/`invokeAction` methods (previously
    `NotImplementedError` stubs) to the real endpoints/commands.
- **Action availability decisions** (`fm-application::action::core_actions()`):
  - `core.open`, `core.openWith`, `core.openTerminal`, `core.copyPath`, `core.copyRelativePath` →
    `unimplemented()` (`feature_available: false`): no backend implementation exists yet for
    opening files/terminals or clipboard/relative-path support, so per the acceptance criteria
    they are registered as unavailable rather than omitted.
  - `core.copy`, `core.move`, `core.delete` → `selection()` (requires a non-empty selection).
    `core.delete` maps to the operation engine's `Delete` kind (permanent delete, not
    trash/recycle-bin — no separate "move to trash" operation kind exists in `fm-operations` yet).
  - `core.rename` → `single_selection()`.
  - `core.createDirectory` and the 12 selection/navigation actions from 0028 → `none()` (always
    available; navigation/selection actions don't mutate files and `createDirectory` doesn't need
    a selection).
- **Design decision**: `InvokeActionRequestDto` (and the Tauri `invoke_action` command) take the
  action id as a path parameter / separate command argument, never duplicated inside the request
  body, since the REST route already carries it in the URL.
- **Bug found and fixed**: utoipa's `ToSchema` derive does not honour the serde container
  attributes `rename_all`/`rename_all_fields` for struct-like enum variant fields, so
  `ActionSourceDto::Plugin { plugin_id }`'s generated OpenAPI schema (and therefore the
  Orval-generated TS type) advertised `plugin_id` while the actual wire JSON (governed by serde)
  is `pluginId`. Fixed with an explicit `#[schema(rename = "pluginId")]` on the field; added
  `action_source_dto_plugin_variant_serializes_its_field_as_camel_case` to guard the real wire
  shape against regression.
- **Lesson for future tasks**: any test that calls into the operation `Scheduler` (directly, or
  transitively via `invoke_action` for a mutating action) must be `#[tokio::test] async fn`, not
  `#[test] fn` — `Scheduler::submit` panics without a live tokio reactor. Because operations run
  asynchronously in the background, such tests must poll `get_operation` until a terminal state
  is reached rather than asserting immediately after the call returns (see
  `invoke_action_delegates_create_directory_to_the_operation_engine` in `service.rs` for the
  pattern).
- **Verification**: `cargo test --workspace` — all crates pass (new test count: 7 + 5 + 2 + 1 + 8
  + 7 + 5 = 35 new tests, all passing). One pre-existing, unrelated failure was observed in
  `fm-vfs-local`'s `local_provider::metadata_is_separate_and_capabilities_are_truthful`
  (capability-flag mismatch predating this task; confirmed via `git stash` that it fails
  identically without any of this task's changes) — left untouched, out of scope. `cargo clippy
  --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` are clean. Frontend:
  `tsc --noEmit` clean, `vitest run` — 260/262 passing, with the 2 failures
  (`component-colours.test.ts`'s hard-coded-hex check on `pane.css`, and
  `http-file-manager-client.test.ts`'s `listOperations`/cancellation test) both confirmed
  pre-existing via the same `git stash` check, unrelated to this task. `biome check .` clean for
  all files this task touched (the two pre-existing `pane.css` `!important` warnings are
  unrelated). `pnpm run api:export`/`api:generate` regenerate cleanly with no further drift.
