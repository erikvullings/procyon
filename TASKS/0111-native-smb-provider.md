# 0011 Native SMB provider

Status: open
Priority: low
Subsystem: backend
Depends on: 0003, 0008, 0009

## Context
Optional direct `smb://` browsing without mounting through the OS. Only pursue if 0002 is insufficient for real use cases.

## Acceptance Criteria
- Direct SMB profiles enumerate configured shares.
- Authentication uses `CredentialStore`.
- Browse/read/write/rename/delete capabilities are mapped correctly.
- Locking/reconnect failures are structured.
- Cross-provider transfers use the shared operation engine.
- Change behavior integrates with generalized tracking.
- Isolated SMB fixtures cover macOS/Windows behavior where possible.
- Existing OS-mounted SMB support remains available.

## Implementation Notes
- Document chosen SMB library, dialects and authentication scope before coding.
- Kerberos/DFS/advanced ACL behavior may need separate tasks.
- Native and mounted SMB should coexist.

## Agent Notes
- Validate product demand and Rust SMB ecosystem maturity before changing to `in_progress`.
