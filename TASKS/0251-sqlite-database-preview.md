# 0251 SQLite database preview

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0100, 0142

## Context

Total Commander's SQLiteViewer is among the most-used Lister (WLX) plugins. Procyon's F3 viewer has no
SQLite support. Requested as a read-only table browser: list tables/views, schema, and paged rows.

Decision to make first: **builtin vs. plugin**. Cost evaluation (2026-10-18):

- `rusqlite` (`bundled`) is already a workspace dependency (fm-application, fm-semantic-worker), so a
  builtin adds ~0 binary size and no new supply-chain surface. A Lua/JS plugin would need a pure-script
  SQLite parser (slow, large, bounded-memory hard) - not viable. **Recommendation: builtin.**
- Reuse the structured-view session machinery (`crates/fm-application/src/structured_view.rs`, task 0100):
  paged rows, cancellation, bounded memory. Add a `sqlite` format with a table picker rather than a new viewer.
- rusqlite needs a real file path. Local files open directly; remote/archive VFS entries must be copied to a
  size-capped temp file (reject over the cap with a friendly state).
- Safety is the main cost: open `SQLITE_OPEN_READ_ONLY` + `immutable=1` (no WAL/journal side effects on the
  user's file), forbid extension loading, set an authorizer allowing only SELECT/READ, cap rows, column
  width, blob preview, and per-query time (progress handler). Untrusted DB files are an attack surface.
- Parity: server (Axum) and Tauri both go through fm-application; mock client needs fixtures.
- OpenAPI/Orval regeneration (`pnpm api:export`, `pnpm api:generate`) for the new format/DTOs.

## Acceptance Criteria

- F3 on `.sqlite`/`.sqlite3`/`.db`/`.db3` (magic-checked: `SQLite format 3\0`) opens a table browser.
- Tables/views list, `CREATE` schema, paged rows; BLOBs shown as size + hex head; NULL distinct from empty.
- Strictly read-only: file mtime/bytes unchanged after viewing, including WAL-mode databases.
- Corrupt/encrypted/oversized DBs show a friendly failure state, never crash the viewer or server.
- Resource limits (rows, time, memory, temp-copy size) enforced and tested.
- Tests: Rust (temp roots only), frontend controller/view, mock client; `pnpm api:check` clean.

## Implementation Notes

- Extension claim must not shadow built-ins; `.db` is ambiguous - require magic bytes.
- Re-evaluate if the temp-copy requirement for remote files proves too costly (could fall back to
  "download first" prompt).

## Agent Notes

- 2026-10-18: Task created from the preview-plugin review (0142). Not started.
