# 0317 FTP upload completion and resume

Status: done
Priority: low
Subsystem: backend, remote
Depends on: 0303

## Context
Found while closing 0303/0315: the FTP writer used a bare duplex pipe like S3/WebDAV did, so
`shutdown` returned before the server had stored the file and a refused or truncated `STOR` was
never reported. FTP also could not resume an interrupted upload.

## Acceptance Criteria
- The FTP writer's `shutdown` waits for the server's final reply and fails with its error.
- `open_write_at` continues a partial with `APPE` when it holds exactly the resume offset.

## Agent Notes
- 2026-11-06 claude: DONE. `crates/fm-vfs-ftp/src/provider.rs`: writer built on `fm_vfs::upload_pipe` (the data connection is always finalised so the control connection stays in sync; a cancelled or failed copy reports `Err`). `open_write_at` checks the partial's size equals the offset (else `Io`, a missing partial is `NotFound`) and appends with `APPE`; `random_write` advertised so the existing engine proof (`open_read` of the partial vs the source) applies. Reads restart from the start (`random_read` stays false).
- Tests: 3 new contract tests (shutdown waits without polling, APPE continues, wrong offset/missing partial refused and nothing appended). NOT verified against a real FTP server (no image run; the fixtures implement `STOR`/`APPE`): servers that disable `APPE` or report `SIZE` in ASCII mode would fail the proof or the append and the copy then restarts.
