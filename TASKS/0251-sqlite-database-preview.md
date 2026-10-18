# 0251 SQLite database preview

Status: done
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
- 2026-10-18: Implemented as a **builtin**, beyond the original read-only browser: table/view list, structure
  (`PRAGMA table_info` + `CREATE` text), paged data grid with free-text search over all columns, per-column
  filters (contains/equals/not equals/greater/less/NULL/not NULL) and sorting, plus a read-only SQL console.
  - Backend `crates/fm-application/src/sqlite_view.rs` (`SqliteViewService`, wired in `service.rs`), DTOs in
    `fm-transport-dto/src/sqlite.rs`, REST `POST /api/v1/files/sqlite/{open,query,close}`, Tauri
    `open_sqlite_view`/`query_sqlite_view`/`close_sqlite_view`; OpenAPI + Orval regenerated.
  - Safety: magic bytes checked first (`.db` is ambiguous, so any open failure falls back to the text/hex view);
    `SQLITE_OPEN_READ_ONLY` + `mode=ro&immutable=1` URI (percent-encoded; test proves bytes and directory listing
    unchanged for a WAL database), defensive mode, `trusted_schema` off, `LIMIT_LENGTH` 16 MiB, SQL 64 KiB,
    `LIMIT_ATTACHED` 0, `query_only`; an authorizer admits only SELECT/READ/RECURSIVE, functions except
    `load_extension`, and a whitelist of introspection pragmas (argument forms of header pragmas denied);
    additionally `stmt.readonly()` and single-statement checks. 5 s deadline per query via the progress handler,
    pages capped at 500 rows and ~4 MiB, cells cut at 2048 chars, BLOBs shown as size + 32-byte hex head, NULL is
    distinct from empty text, 64-bit integers are sent as text.
  - Identifiers in browse queries are validated against the schema and quoted; values are bound parameters.
  - Non-local files (SFTP, archives, ...) are copied to a temp file capped at 512 MiB, deleted on close; max 16
    sessions. A `-wal` file next to a local database yields a warning because `immutable` ignores uncheckpointed
    writes.
  - What bundled rusqlite already gives for free (verified in tests): FTS5 `MATCH`, JSON1, window functions,
    recursive CTEs, R-tree and the `dbstat` virtual table all work in the SQL console. Enabled extra rusqlite
    features: `hooks` (authorizer, progress handler) and `limits`.
  - Frontend: `preview/sqlite-browser.ts` (state + loaders), `sqlite-view.ts` (+ CSS in `file-viewer.css`),
    controller kind `sqlite` with debounced search/filters, stale-response guards and session close on dispose;
    mock client has an in-memory two-table database (`mock-sqlite-database.ts`, SQL limited to `SELECT * FROM t`).
    i18n keys in all 8 languages.
  - Not verified: Windows/Linux (URI path encoding for drive letters is unit-tested only); the mock dev runtime has
    no `.sqlite` file in its fake directory, so the feature is only reachable there by opening such a name.
  - Not done: editing (by design read-only), exporting results, multi-database ATTACH, SQLCipher.
- 2026-10-18 (code review): pager advances by rows shown; a broken view no longer blocks opening (listed without
  columns); numeric filter input binds as a number and empty filters are ignored; session cap counts in-flight
  opens; the total count gets its own time budget and runs after the page; non-"not a database" open failures
  are shown instead of falling back to hex.
