# 0039 Operation: copy a single file

Status: done
Priority: high
Owner: unassigned
Agent: Codex
Area: backend
Depends on: 0038

## Context
`file-manager-coding-agent-spec.md` §33 step 7 item 3 and §17 safety requirements. Single-file copy
establishes the streaming, temp-name and metadata-preservation pattern that directory copy reuses.

## Acceptance Criteria
- `OperationKind::Copy` for one file, streaming through the provider's read/write streams without
  loading the file into memory.
- Copies to a temporary destination name, then performs a final atomic rename (§17).
- On cancellation or failure, the temporary file is removed — no partial destination files remain
  (integration test asserts this).
- Byte and item progress reported and throttled (§28).
- Handles: source disappearing mid-copy, destination appearing after planning, disk full,
  permission denied, locked files (Windows), zero-byte files, and very large files.
- Timestamps preserved; permissions preserved where the platform supports it. `docs/architecture/`
  documents exactly which metadata is preserved and which is not (§17).
- Conflict detection reports a conflict rather than overwriting; policy handling arrives in 0045, so
  for now anything other than an explicit `overwrite`/`renameNew` fails safely.
- `F5` copies the selection to the other pane (single file for now).
- Integration tests cover every bullet above using temp directories only.

## Implementation Notes
- Use a copy-on-write / server-side clone fast path where the platform offers one
  (`clonefile` on APFS, `FSCTL_DUPLICATE_EXTENTS` on ReFS), falling back to streaming; keep it
  behind the `SERVER_SIDE_COPY` capability flag (§6).
- Sparse-file preservation is best-effort; document the behaviour rather than claiming support.

## Agent Notes
- Implemented a provider-backed single-file copy executor with a bounded 128 KiB streaming
  fallback, private temporary destination, cancellation/failure cleanup, and atomic publication.
- Added an APFS clone fast path behind `SERVER_SIDE_COPY`; unsupported or failed clones fall back
  to streaming. ReFS cloning is not advertised because Rust's standard library does not expose a
  safe implementation. Sparse preservation remains best-effort and is documented.
- Added safe collision handling for `overwrite` and `renameNew`; all unresolved policies fail
  without replacing the destination. Metadata preservation covers modified/accessed timestamps and
  platform permissions; unsupported metadata is listed in
  `docs/architecture/file-copy-metadata.md`.
- Wired F5 to copy exactly one selected file to the other pane through the shared application API,
  preserving browser/Tauri parity.
- Added provider and application integration coverage for streamed temporary copies, zero/large
  files, cancellation cleanup, missing sources, destination races, overwrite/rename-new, progress,
  and metadata, plus a focused F5 component test. OS I/O errors (including disk-full, permission,
  and locked-file errors) flow through the same typed failure and cleanup path; Windows-specific
  locked-file behavior was not executable on the macOS development host.
- Verification: `cargo check` and `cargo clippy -- -D warnings` passed for all affected crates;
  `fm-vfs-local`, `fm-operations`, and copy-operation tests passed; TypeScript checking and the
  focused F5 test passed. The complete `app-shell.test.ts` file still has one pre-existing keyboard
  selection failure (`keeps cursor and selection independent...`, expected 4 rows but received 5).
