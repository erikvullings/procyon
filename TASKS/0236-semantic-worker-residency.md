# 0236 Keep the semantic worker warm while semantic search is enabled

Status: done
Priority: high
Subsystem: backend, rag
Depends on: 0235

## Context

Opening Knowledge search after Procyon has been idle is slow. The semantic worker
(`fm-semantic-worker`) is spawned lazily by `IpcSemanticCapability::worker_client()`; it loads the
embedding model and opens Zvec before it serves. Its server closes an authenticated connection that
sent no frame for `STREAM_DEADLINE` (5 min) and then exits after a 30 s idle timeout, so the worker
dies about 5.5 min after its last use and every later open pays a cold start. A warm worker
measured about 750 MB RSS with a 974 MB index and the 465 MB e5-small model; Zvec ran with library
defaults (`initialize(None)`), i.e. no memory cap.

`zvec-ai/zvec-grep` keeps a long-lived local daemon, retires idle runtimes on a timer (model
15 min, workspace 4 h, configurable) and initialises Zvec with an explicit soft memory cap.

## Acceptance Criteria

- When semantic search is enabled (a semantic library is available with at least one enrolled
  root), the desktop host warms the worker in the background after startup. Warm-up never blocks
  window creation or other startup work.
- While enabled and used within a residency window (4 h after startup or the last search), the
  host keeps the worker alive with a periodic health request shorter than the stream deadline.
  After the window lapses the keepalive stops and the worker exits by its own idle policy; the
  next search restarts it and residency resumes.
- Warm-up/keepalive failures are logged and retried on the next tick; they never surface as user
  errors or spawn a tight retry loop.
- Zvec is initialised with an explicit soft memory cap.
- Browser/server hosts are unaffected (administrator-provisioned workers keep their own lifetime).

## Implementation Notes

- Residency lives in `fm-application` (`semantic_residency.rs`), driven by `SemanticService`
  activity timestamps; the Tauri setup only spawns it. Keepalive reuses the cached worker
  connection, so a health frame also resets the server-side stream deadline.
- Out of scope: retiring only the model inside a live worker, a user-facing residency setting.

## Agent Notes

- 2026-10-04 Copilot: User chose this together with 0237 after comparing with zvec-grep; asked that
  warm-up run in the background and not delay startup.
- 2026-10-04 Copilot: Done. `SemanticService` records search activity (`query`,
  `knowledge_search`, `knowledge_capabilities`); `semantic_residency::keep_resident` warms
  immediately and pings `health()` every 2 min within a 4 h window, consulting library status each
  tick so it never spawns a worker without enrolled roots. Tauri spawns it via
  `FileManagerService::keep_semantic_worker_resident` only for managed components, stopping on the
  shared shutdown token. Zvec now initialises with a 1 GiB soft cap (threads left at library
  default so indexing is not slowed; the cap is not yet measured against large libraries). Because
  `zvec_storage.rs` is fingerprinted, `docs/evaluations/knowledge-retrieval-v1.json` was
  regenerated. Paused-time unit tests cover warm-up, window lapse/resume, disabled, failure retry
  and shutdown; the real-worker lifetime was verified only manually in `tauri dev`.
