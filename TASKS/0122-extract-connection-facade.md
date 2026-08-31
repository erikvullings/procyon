# 0122 Extract Connection Facade

Status: done
Priority: medium
Subsystem: backend
Depends on: 0119

## Context

`FileManagerService` has 6+ connection methods (lines ~2041-2215), each repeating the same 3-line pattern: call `ConnectionService` method → fetch status → fetch last_error → convert to DTO via `connection_dto::connection_dto()`. These are shallow pass-throughs that fail the **deletion test** — deleting them just moves the same pattern to each caller.

The `connection_dto.rs` module already exists and handles the conversion correctly. The facade should own the full "call → assemble DTO" pattern.

## Acceptance Criteria
- `ConnectionFacade` module wrapping `ConnectionService` and exposing methods that return `ConnectionDto` directly
- The "call service → fetch status → fetch last_error → convert to DTO" pattern lives in ONE place per method
- SSH host-key probe/accept methods included in the facade
- `FileManagerService` delegates the entire connection feature to a single `connections` field (replacing 6+ inline methods)
- `connection_dto.rs` remains as the conversion helper (it's already good)
- Zero behavioural changes

## Implementation Notes
- The facade needs: `ConnectionService`, `SshConnectionManager` (for host-key probing)
- This is the quickest refactor — ~175 lines removed from `service.rs`
- The `ConnectionDraft` construction from DTO should move into the facade too, removing the inline `ConnectionDraft { ... }` blocks in `create_connection` and `update_connection`
- Consider whether `probe_ssh_host_key` and `accept_ssh_host_key` logic (SSH config extraction, re-probe verification) should also move here

## Agent Notes

- 2026-08-11 Claude: Implementation already complete. Verified `connection_facade.rs` (261 lines) wraps `ConnectionService` and `SshConnectionManager`, exposing 11 methods that return `ConnectionDto` or `HostKeyProbeDto` directly. The "call → fetch status → fetch last_error → convert to DTO" pattern lives in one place per method. `ConnectionDraft` construction for create/update is encapsulated in `connection_draft_from_create`/`connection_draft_from_update` helpers. SSH host-key probe/accept logic (re-probe verification, RequireKnownHost guard) is included. `FileManagerService` delegates all connection operations to a single `connections` field. `connection_dto.rs` remains as the conversion helper. Zero behavioural changes. Compiles cleanly, all 161 unit tests pass.
