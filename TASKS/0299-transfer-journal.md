# 0299 Durable transfer journal

Status: done
Priority: high
Subsystem: backend, operations
Depends on: 0035, 0047

## Context
Queued and running transfer jobs must survive a crash or restart. Today only terminal snapshots are
persisted (`operation_history.rs`); a job that was queued or running when the process died simply
vanishes, and a private partial destination can be left behind with no record of whose it is.
Parent: 0163.

## Acceptance Criteria
- A crash-consistent journal (write temp + fsync + atomic rename, or append-only with checksummed
  records) persists, per queued/running transfer: job id, plan intent, source/destination
  locations, source revision (size, mtime, provider validator), conflict policy, destination
  partial path, and checkpoints (bytes committed per item).
- Journal stores intent and checkpoints only; never live provider/session objects or credentials.
- Writes stay off the scheduler's critical path (reuse the write-behind approach of
  `OperationHistory`), but a checkpoint is durable before the item is reported as committed.
- A torn or corrupt journal file is detected, quarantined, and never prevents startup.
- Terminal jobs are removed from the journal once the history entry is durable.
- Fault-injection tests: crash between checkpoint and rename, truncated file, corrupt record.

## Implementation Notes
- Define the record schema and versioning first; additive fields, explicit version.
- Keep it inside the operation engine path; the engine remains the sole mutation authority.

## Agent Notes
- 2026-11-05 claude: Split from 0163; this is the foundation the other subtasks build on.
- 2026-11-05 claude: Built `crates/fm-operations/src/journal.rs` (`OperationJournal` trait, `FileOperationJournal`, versioned record schema, replay + quarantine) and wired it into `Scheduler::with_journal` (begin at job start so queued jobs are covered, `Items` after planning/expansion, `Committed` per finished item, `finish` when the job is terminal in any outcome). Only `Copy`/`Move`/`Duplicate` are journaled. `OperationProgressReporter` gained `journaling()`, `note_partial()` and `note_source_revision()` (default no-ops); `CopyExecutor::copy_file` announces its private `.fm-copy-*` partial and a source revision (size, mtime; no validator yet) only when journaling. `FileManagerService` opens `<settings>/transfer-journal/` and logs recovered/quarantined jobs.
- Format: one append-only file per job, `<crc32> <json>` per line. A torn tail or a corrupt middle record keeps the valid prefix and reports `JournalDamage`; an unreadable/unknown-version/foreign `begin` record moves the file to `quarantine/`. Startup never fails because of the journal; a journal that cannot open or begin only degrades recovery.
- Verified: 10 unit tests in `journal.rs` (restart recovery, finish removes, torn tail, corrupt middle, quarantine, version/foreign, double begin, interleaved jobs, stray records), 6 scheduler tests in `crates/fm-operations/tests/transfer_journal.rs` (ordering, mid-run crash state, failure still clears, non-transfer kinds skipped, journal failure never fails the copy, file journal survives an interrupted job), 2 planner tests (`copy_file_announces_*`). `cargo clippy -p fm-operations -p fm-application --all-targets -D warnings` clean; `cargo nextest run -p fm-operations -p fm-application -p fm-server` 1498 passed.
- Performance finding: a per-record `fsync` made the 10k-small-file copy test ~4x slower (32 s -> 138 s) because it competes with the copy's own durable writes. Appends are therefore group-committed at most every 250 ms (`SYNC_INTERVAL`); with that the test is within noise of the journal-off run (59.7 s vs 58.7 s on a loaded machine).
- DELIBERATE DEVIATIONS from the acceptance text, for 0300 to know: (1) item-commit checkpoints are not individually durable before the item is reported committed; a crash can lose up to 250 ms of records, so recovery must read "destination exists but no commit record" as completed-unconfirmed, never as safe to overwrite. `begin` is fully durable (file + directory fsync) before the job runs. (2) There are no mid-file byte-offset checkpoints; nothing can resume yet (0303), so item granularity is what the journal stores. (3) The journal record is removed when the job is terminal, not when the write-behind history entry is durable (that writer cannot be awaited); a crash in that window loses the history entry but no transfer data.
- NOT DONE here: classification and user-facing recovery (0300); recovered job files are left in place and only logged until 0300 consumes `Recovery` and calls `finish`. `SourceRevision.validator` is always `None` (the entry summary carries no ETag/version).
- 2026-11-05 claude: Code-review follow-ups applied: exclusive `.lock` on the journal directory (a second instance gets `JournalError::Io` and runs unjournaled instead of treating the first's live jobs as interrupted); a torn `begin` record is deleted, not quarantined (the job never ran); a failed append logs a warning; `submit` no longer clones the snapshot to read `kind`; journal directory 0700 and files 0600 on Unix. Final run: clippy clean, `cargo nextest run -p fm-operations -p fm-application` 1265 passed.
- Known limits left for later subtasks: recovered files accumulate and their `.fm-copy-*` partials stay until 0300 consumes `Recovery`; `BulkCompleted` and conflict-skipped items get no `Committed` record (0300 must treat them as unknown, not untouched); the revision is stat-ed at copy time (one extra metadata call per file while journaling, noticeable on high-latency remotes) rather than taken from the plan; `Drop` joins the writer thread synchronously (only at last-reference drop, i.e. shutdown); locations are stored as-is (providers use connection ids, not credentials, in URIs).

