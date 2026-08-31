# 0009 Remote change tracking

Status: done
Priority: medium
Subsystem: backend
Depends on: 0004, 0006

## Context
Generalize directory change tracking so remote providers can use polling or future delta APIs instead of pretending to support native filesystem watch semantics.

## Acceptance Criteria
- Provider change tracking is explicit: native watch, delta API, poll, or unsupported.
- Local behavior remains native watch.
- SFTP/FTP can use conservative polling.
- Polling is cancellable and stops with the directory session.
- Inactive/background tabs can poll less frequently.
- Failures back off.
- Unchanged polls do not emit unnecessary revisions/redraws.
- SSE/Tauri event behavior stays transport-neutral.
- Tests cover polling lifecycle and backoff.

## Implementation Notes
- Introduce `ChangeTracking` or equivalent.
- Keep provider-specific refresh behind the directory service.
- Do not create per-row/per-file timers.
- Design so future native OneDrive can use delta tokens without another redesign.

## Agent Notes
- Inspect directory-session cancellation/lifecycle before adding timers.
- 2026-08-14 claude: Note on the "Depends on" line above: `0004`/`0006` are this task's own
  in-file numbering (visible in its `# 0009 Remote change tracking` heading vs. the `0109-`
  filename), not `TASKS/0004`/`TASKS/0006`. They resolve to `TASKS/0104-sftp-provider.md`
  ("0004 SFTP provider") and `TASKS/0106-ftp-ftps-provider.md` ("0006 FTP and FTPS provider"),
  both already `done` — confirmed by reading both task files before starting.
- 2026-08-14 claude: Implemented end to end with TDD. New `fm_vfs::ChangeTracking` enum
  (`NativeWatch` / `DeltaApi` / `Poll { interval }` / `Unsupported`) plus a
  `FileSystemProvider::change_tracking()` default method (derived from the `WATCH` capability bit)
  in `crates/fm-vfs/src/change_tracking.rs`, distinct from `ProviderCapabilities::WATCH` (which
  only says `watch()` can be called at all, not how a caller should treat its absence).
  `SftpFileSystemProvider`/`FtpFileSystemProvider` override it to `Poll` at a new
  `fm_vfs::CONSERVATIVE_POLL_INTERVAL` (20s) — their `capabilities()`/`watch()` are unchanged
  (still no `WATCH` bit, still `UnsupportedCapability`), so this adds tracking without faking watch
  semantics, per 0104/0106's own explicit notes ("real polling is task 0109's job",
  "Do not fake watch... semantics").
- 2026-08-14 claude: `fm-application/src/directory.rs`'s `WatchHub` is the only place that branches
  on `change_tracking()`; `DirectoryService::list()`'s watch-acquisition gate changed from checking
  the `WATCH` capability to `change_tracking() != Unsupported`. For `NativeWatch`/`DeltaApi` it
  still calls `provider.watch()` unchanged (local behavior is untouched, confirmed by the full
  `fm-vfs-local` and existing `directory.rs` watch-lifecycle suites staying green). For `Poll`, a
  new `poll_change_stream()` builds a `ProviderChangeStream` by re-listing on an interval and
  comparing entries by id (not list order — providers make no ordering guarantee) via a `HashMap`
  fingerprint; the first tick only seeds a baseline and never emits (the pane's initial listing
  already reflects it), so an unchanged poll never bumps a revision or triggers a redraw. This
  stream feeds the *same* shared-by-location, reference-counted `SharedWatch` and per-pane
  diffing/publish path a native watch already used, so polling is cancellable and stops with the
  directory session for free (navigating away releases the last reference, cancelling the
  poller's `CancellationToken`, verified by a test that the provider's `list()` call count stops
  growing). A failed poll doubles the wait (capped at 8x) rather than tearing down the watch, so a
  transient network hiccup doesn't force a full reset; verified via a test asserting successive
  poll-attempt gaps grow after consecutive failures. SSE/Tauri stayed untouched — `EventBus`/
  `DirectoryDeltaPayload` publishing is identical regardless of tracking kind, satisfying transport
  neutrality by construction rather than by a new code path.
- 2026-08-14 claude: "Inactive/background tabs poll less frequently" — added
  `DirectoryService::set_pane_activity(pane_id, active)`, backed by a per-location
  `Arc<AtomicBool>` on `SharedWatch` (shared by every pane watching that location; last write
  wins, a documented simplification since two panes on the exact same remote directory
  simultaneously is a rare edge case the acceptance criteria doesn't call out) read fresh by the
  poller before each sleep, throttling a poll-tracked location to 4x its base interval. Exposed as
  `POST /api/v1/directories/activity` (`setPaneActivity`, 204/404) and the mirrored
  `set_pane_activity` Tauri command, both thin wrappers over the same service method (host parity).
  A no-op for a pane with no watch registered (no change tracking, or its listing hasn't finished
  yet); only an unknown pane id is rejected. **Known, explicitly out-of-scope gap**: no frontend UI
  wires `document.visibilitychange` (or similar) to call this endpoint yet — this task is
  `Subsystem: backend`, and the mechanism is real, tested, and ready for a future task to call, the
  same documented-gap pattern task 0104 used for its host-key-confirmation dialog. The frontend
  client interface/Http/Tauri/Mock implementations were still added for host parity and so the
  capability isn't stranded behind an incomplete client surface.
- 2026-08-14 claude: `fm_transport_dto::SetPaneActivityRequest` (`paneId`, `active`) added to
  `requests.rs` alongside its siblings; OpenAPI/Orval regenerated (`pnpm run api:export` then
  `api:generate`), re-run a second time to confirm byte-identical output (no drift). Frontend
  `frontend/src/models/requests.ts` gained the mirrored model type; `FileManagerClient` gained
  `setPaneActivity`, implemented in `Http`/`Tauri`/`Mock` clients (mirroring `getEntryMetadata`'s
  shape) — required so all three implementations still satisfy the interface, not itself part of
  any UI change.
- 2026-08-14 claude: Verified (exact commands, not whole-suite totals): `cargo test -p fm-vfs
  --test provider_contract` → 7 passed (2 new: default `ChangeTracking` derivation with/without the
  `WATCH` bit); `cargo test -p fm-vfs-sftp --test provider` → 19 passed (1 new); `cargo test -p
  fm-vfs-ftp --test provider_contract` → 6 passed/2 ignored-networked (1 new); `cargo test -p
  fm-transport-dto` → 1 new (`set_pane_activity_request_round_trips...`) among 95 passed; `cargo
  test -p fm-application --lib directory::` → 15 passed (7 new: change-produces-a-delta,
  unchanged-polls-emit-nothing, backoff-grows-on-failure, navigating-away-stops-polling,
  unsupported-tracking-is-never-watched, activity-throttling-slows-the-cadence,
  activity-on-an-unknown-pane-is-rejected — each re-run 3x individually with no flakes observed);
  `cargo test -p fm-server --test directory_routes` → 4 passed (1 new end-to-end
  204/404 test, plus the existing stable-operation-id test extended with `setPaneActivity`). Full
  regressions: `cargo test --workspace` → 561 passed, 1 unrelated pre-existing failure
  (`fm-plugin-runtime`'s `discovers_the_real_catppuccin_icons_plugin_package`, confirmed to fail
  identically on a clean `git stash` of this task's changes — nothing this task touches); `cargo
  clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` both clean.
  Frontend: `pnpm run typecheck` clean, `pnpm run lint:frontend` clean (one pre-existing,
  unrelated `theme.css` specificity warning), `pnpm exec vitest run` → 1001 passed / 95 files (no
  new frontend tests added — the client wiring has no UI consumer yet per the gap noted above, so
  nothing new to unit-test beyond type-checking the interface implementations). `pnpm run
  test:scripts` → 39 passed (architecture-docs/CI-workflow fitness tests, unaffected).
- 2026-08-14 claude: Docs updated for the surface actually added:
  `docs/architecture/filesystem-watching.md` gained a "Remote change tracking (task 0109)" section
  describing the four `ChangeTracking` kinds, the poll/backoff/activity mechanics, and how
  cancellation is shared with the native-watch lifecycle. `README.md`'s remote-connections
  paragraph gained a short note on SFTP/FTP polling and `set_pane_activity`, and (found while
  editing that exact paragraph) fixed a stale claim that "FTP/FTPS ... remain unimplemented" —
  task 0106 already implemented it; only native cloud/SMB genuinely remain unimplemented.
