# 0293 Reduce FileManagerService composition root from 95 public methods

Status: open
Priority: medium
Subsystem: backend
Depends on: none

## Context

`crates/fm-application/src/service.rs` is 8231 lines and defines `FileManagerService` with 95 public methods. It serves as the host-facing composition root but has grown into a coordination layer without behavioral depth. Every caller learns a massive contract. The builder pattern (`with_semantic_library_service`, `with_document_summary_capability`, etc.) makes construction even more complex — there are 10+ `with_*` builder methods for optional capabilities.

## Acceptance Criteria

- `FileManagerService` is reduced to under 50 public methods
- Capability-specific methods are moved into dedicated capability modules (e.g. `semantic_library.rs`, `checksum_coordinator.rs`)
- The composition root delegates to capability modules without exposing their internals
- A capability configuration struct replaces the 10+ builder methods
- `cargo test --workspace` passes with no regressions
- `cargo clippy --workspace` passes with no new warnings

## Implementation Notes

- The capability modules already exist (e.g. `checksum_coordinator.rs`, `search_comparison_coordinator.rs`, `operations_coordinator.rs`) — the service should delegate to them, not re-export their methods
- Builder methods for optional capabilities should be replaced by a single `ServiceConfig` struct with optional fields
- The `FileManagerService` should focus on: workspace lifecycle, action dispatch, event publication, and capability orchestration
- Methods that are thin delegations to capability modules should be removed from the service interface
- Keep the service at a level that makes sense for host-facing orchestration, not capability-level detail

## Agent Notes

- The 95 public methods include many thin delegations to capability modules — measure which callers actually use the service vs. the capability directly
- The `with_*` builder pattern is the real problem — it exposes capability injection as public API when it should be internal to construction
- The spec says `FileManagerService` is the "composition root, not the implementation home for new backend features" — so reducing its public surface aligns with the spec
- Consider whether some capability modules should be directly injectable rather than going through the service
- This is a high-risk change — make sure every removed method still works through a capability module path
