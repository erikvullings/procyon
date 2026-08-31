# Filesystem watching and remote change tracking

Open local directories are watched through `fm-vfs` provider invalidations. Providers do not
construct `DirectoryDelta` values: `fm-application` owns the pane snapshot, revision, filtering,
sorting, diffing, workspace routing, and reset policy.

`fm-vfs-local` uses notify's polling watcher with a bounded callback channel and a 75 ms debounce.
Polling is intentional for the first cross-platform implementation: it gives Linux, macOS, and
Windows the same overflow semantics and avoids depending on a host event-loop integration. A
callback-channel overflow or notify rescan flag becomes `ResetRequired`; the application then
lists the directory again and publishes `DirectoryDelta::Reset` with a fresh snapshot.

Native watcher backends may replace polling after regular Tauri testing is available. Their
platform behavior must remain hidden behind the same invalidation contract:

- macOS FSEvents coalesces changes and can report only that a directory changed. It can also report
  a historical-event gap, which must map to `ResetRequired`.
- Windows `ReadDirectoryChangesW` uses a finite kernel buffer. Buffer overflow loses filenames and
  must map to `ResetRequired`, never a guessed incremental delta.
- Linux inotify reports queue overflow explicitly and has per-user watch limits. Both conditions
  require reset or a surfaced watch-start failure.

Registrations are shared by location and reference-counted. Each pane independently diffs its
filtered/sorted snapshot; the provider watcher is cancelled when the last pane leaves.

Local `EntryId` values are UUIDv5 values derived from filesystem identity (`device + inode` on
Unix, volume serial + file index on Windows), so a rename is emitted as an update rather than a
remove/add pair.

## Remote change tracking (task 0109)

`FileSystemProvider::change_tracking` reports one of four `fm_vfs::ChangeTracking` kinds, kept
distinct from the `WATCH` capability bit (which only says `watch` can be *called* at all):

- `NativeWatch` — `watch` streams real OS filesystem events (the local provider's default, derived
  automatically from the `WATCH` capability).
- `DeltaApi` — `watch` streams notifications derived from a remote delta/sync-token API rather than
  an OS event source; reserved for a future native OneDrive provider (task 0110) so it can plug in
  without another redesign of this abstraction.
- `Poll { interval }` — no push notifications exist; `fm-application`'s `WatchHub` polls `list`
  itself and diffs the result, rather than the provider faking a `watch` stream it cannot honestly
  implement. SFTP (task 0104), FTP/FTPS (task 0106), WebDAV (task 0147) and S3 (task 0146) all
  report this, at
  `fm_vfs::CONSERVATIVE_POLL_INTERVAL` (20s).
- `Unsupported` — no change tracking at all (e.g. search results, archives); the location is never
  watched or polled, matching the pre-0109 behavior for such providers.

If a provider advertises `NativeWatch` or `DeltaApi` but cannot start its watch, `WatchHub` records
the failure and falls back to one-second polling. This is intentionally faster than normal remote
polling because it is an error-recovery path for a folder the user currently has open.

For `Poll` providers, `WatchHub::acquire` builds a `ProviderChangeStream` via
`directory::poll_change_stream` instead of calling the provider's `watch`. Each tick re-lists the
directory and compares entries by id (not list order, since a provider makes no ordering
guarantee); the first tick only seeds a baseline and never emits, since the pane's initial listing
already reflects it. A changed listing emits `ProviderChange::Changed`, which flows through the
same shared-by-location, reference-counted `SharedWatch` and per-pane diffing/event-publishing path
as a native watch — an unchanged poll never reaches that path, so it never bumps a pane's revision
or triggers a redraw.

A failed poll doubles the wait before the next attempt (capped at 8x the base interval) rather than
tearing down the watch, so a transient network hiccup does not force a full directory reset. A pane
not currently in the foreground can be marked inactive via `DirectoryService::set_pane_activity`
(`POST /api/v1/directories/activity` / the `set_pane_activity` Tauri command), which throttles a
poll-tracked location to 4x its normal interval; the flag is shared by every pane watching that
location (last write wins) and is read fresh before each sleep, so toggling it takes effect on the
very next tick. It has no effect on `NativeWatch`/`DeltaApi` tracking, which is push-based and has
no cadence to throttle. Polling stops the same way a native watch does: cancelling the pane's
`watch_cancellation` on navigation-away releases the shared `SharedWatch` once its reference count
reaches zero, cancelling the underlying poll loop's `CancellationToken`.
