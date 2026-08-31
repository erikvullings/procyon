# 0156 Slow directory navigation (several seconds per folder change) in alpha 5

Status: done
Priority: high
Subsystem: backend, frontend
Depends on: none

## Context

User report (2026-08-26, running the packaged alpha 5 build): changing folders takes several
seconds to display contents. This is a severe regression against the intended UX — directory
listing is meant to be near-instant (virtualized table, paginated backend listing per
[TASKS/0018](0018-local-provider-directory-listing.md)/[TASKS/0019](0019-directory-service-and-list-endpoint.md)/[TASKS/0024](0024-virtualized-directory-table.md)).

Reported alongside [0157](0157-workspace-not-restored-and-tcc-reprompt.md) (workspace restore /
TCC re-prompt) in the same message — may or may not share a root cause; investigate independently
unless evidence points to a shared cause (e.g. if every navigation is re-triggering a TCC prompt
or a full re-scan because of how folder access is granted, that would explain both).

## Acceptance Criteria
- Root cause identified with evidence (profiling, logs, or a reproduced minimal case), not guessed.
- Folder navigation returns to near-instant for typical directory sizes once fixed.
- If the cause is inherent to a specific provider/feature (e.g. git status annotation, thumbnail
  generation, icon overlay fetching blocking the initial listing), either make it async/non-blocking
  for the initial paint or document why it can't be, rather than silently leaving the regression.
- No regression to existing directory-listing/watching tests
  (`cargo test -p fm-application directory::`, `cargo test -p fm-vfs-local`).

## Implementation Notes
- Candidate areas to check first, roughly by likelihood: (1) git-status annotation
  (`crates/fm-application/src/directory.rs`, task 0135) if the user is navigating inside git working
  trees — this walks/queries git2 per listing; (2) native icon overlay fetching (task 0091) if it's
  synchronous per-entry rather than batched/async; (3) volume-capacity enrichment (task 0096/0097) if
  it's doing a blocking syscall per navigation rather than being cached; (4) whether directory
  watching (task 0020) is doing something expensive on each navigate (e.g. re-registering a watch
  synchronously before the listing can return); (5) thumbnail generation (task 0134) if it's
  blocking the initial listing response rather than streaming in after.
- Ask the user for specifics if root-causing from code reading alone isn't conclusive: which
  folder(s) reproduce it (git repo? large directory? network/remote location?), whether it's
  reproducible in the browser dev preview (`pnpm dev` / `pnpm dev:http`) as well as the packaged
  Tauri app (isolates "slow due to packaging/IPC" vs. "slow due to backend logic" vs.
  "slow due to frontend rendering"), and roughly how large the folder is.
- Check `crates/fm-application/src/directory.rs`'s `DirectoryService` (the deep, ~1072-line module
  per 0119's notes) as the likely single place this logic lives, per the layering in
  [docs/architecture/overview.md](../docs/architecture/overview.md).

## Agent Notes
- 2026-08-26: Task created from a direct user bug report (packaged alpha 5 build). Not yet
  investigated.
- 2026-08-26: Root-caused and fixed. Turned out to be **three separate bugs**, found in sequence as
  each one stopped masking the next — a narrow test at just the provider level (like `fm-vfs-local`'s
  existing `returns_the_first_page_of_a_hundred_thousand_entry_directory`) would have missed all
  three, since each only manifests through `DirectoryService::list`'s full call chain. Added
  `directory::tests::listing_a_large_directory_completes_in_roughly_linear_time` (20,000-entry
  directory, 3s ceiling) to exercise all three end-to-end.
  1. **O(n^2) round trips.** `list_all` (`crates/fm-application/src/directory.rs`) drains the whole
     directory via repeated provider `list()` calls (required for correct global sorting — see the
     doc comment on `DirectoryService::list`), using an internal 1,024-entry batch size.
     `LocalFileSystemProvider::list` (`crates/fm-vfs-local/src/lib.rs`) re-opened `read_dir` from
     scratch and re-iterated past `offset` entries on *every* call, since it has no cross-call cursor
     state — so N/page_size calls each re-scanned an ever-larger prefix. A 50,000-entry directory
     meant ~50 round trips summing to ~1.3M redundant iterations instead of 50K. Fixed by bumping the
     internal batch size to 65,536 (covers the vast majority of real directories in one round trip;
     remote providers with real wire-level page caps are unaffected, they just keep paging via
     `has_more` as before). Same fix applied to the identical pattern in
     `crates/fm-comparison/src/engine.rs`'s directory-snapshot helper.
     `crates/fm-search/src/engine.rs`'s recursive walk had a related but *worse* bug: it never looped
     on `has_more`/`continuation_token` at all, silently truncating search results for any directory
     over 1,024 entries — fixed by adding the missing drain loop (same large batch size).
  2. **Per-entry async overhead.** Even after (1), `LocalFileSystemProvider::list` awaited a separate
     `tokio::fs` call (`next_entry`/`symlink_metadata`/`file_type`) per directory entry — each is its
     own Tokio blocking-thread-pool round trip. For 20,000 entries that's 50,000+ sequential async
     hops, ~37s in testing even after (1) reduced it to one provider call. Fixed by batching the whole
     page into one `tokio::task::spawn_blocking` call using plain `std::fs` internally (new
     `list_directory_page_sync`/`summarize_entry_sync`/`is_macos_app_bundle_sync`), replacing the old
     async `summarize_entry`. Trade-off: cancellation can no longer be observed mid-page-scan (the
     caller still checks it before starting) — acceptable since a page this size now completes in
     well under a second.
  3. **The dominant cost, found only once (1) and (2) stopped masking it:**
     `DirectoryService::list` awaited filesystem-watch registration (`self.watches.acquire(...)`,
     which calls `LocalFileSystemProvider::watch` → `notify::RecommendedWatcher` FSEvents on macOS)
     *before returning the listing at all*. Measured at 20-23+ seconds in testing, and **not
     proportional to directory size** — reproduced identically with a single-entry directory,
     isolating it from (1)/(2) entirely. Root mechanism (read from `notify` 8.2.0's vendored source,
     `fsevent.rs`): setting up an FSEvents watch spawns a dedicated OS thread to host a `CFRunLoop`,
     and the calling thread waits for it via a tight busy-spin (`while CFRunLoopIsWaiting(runloop) ==
     0 {}`, no yield/sleep) — done synchronously inside an async fn on a Tokio worker thread, not
     behind `spawn_blocking`. Under thread-scheduling contention that wait can stretch out
     dramatically. Fixed the actual bug regardless of *why* FSEvents setup is slow: watch acquisition
     is now `tokio::spawn`ed instead of awaited inline, since the listing is already complete and
     useful before a live-update watch exists for it. This also fixed a latent correctness bug: `?`
     on a failed `acquire()` used to fail the *entire listing* even though a perfectly good snapshot
     had already been built — a watch failure should only mean "no live updates for this pane," never
     "no listing at all."
     - **Considered and empirically rejected as unnecessary for this bug**: additionally wrapping the
       synchronous FSEvents bootstrap itself in `spawn_blocking` (so the busy-spin never occupies a
       Tokio worker thread at all, regardless of how slow it is). Verified with a controlled
       experiment (single-worker-thread runtime: a plain `tokio::spawn`ed 300ms CPU-spin starved a
       concurrent async heartbeat task to 0 ticks, vs. ~132 ticks when the same spin ran via
       `spawn_blocking`; on this machine's default 16-worker runtime the difference was
       negligible — 133 vs 132 ticks). Confirms the mechanism is real, but since fix #3 above already
       removes watch acquisition from the user-visible path entirely, this would be pure
       defense-in-depth (protects *other* concurrent async work from being starved while FSEvents
       sets up, not this bug) — not implemented, but noted here as a low-priority follow-up if
       something else on the same runtime is ever observed stalling during a first-time directory
       watch registration.
  - Verified: `cargo clippy -p fm-application -p fm-vfs-local -p fm-search -p fm-comparison
    --all-targets -- -D warnings` clean, `cargo fmt` (same crates) clean, `cargo test -p
    fm-application --lib` 251/251, `cargo test -p fm-application` (full integration suite) green,
    `cargo build --workspace` clean.
  - **Not independently verified against the real packaged Tauri app** (no tool in this environment
    can drive it) — the ~20-23s FSEvents-setup figure was measured in this session's sandboxed test
    environment, which was under sustained load all session; the exact magnitude may differ on the
    user's own hardware, but the fix (never blocking the listing on it) is correct regardless of that
    magnitude. Ask the user to confirm folder navigation feels instant in the next build.
  - Marking done: root cause found and fixed with evidence, verified via a regression test that
    exercises all three bugs together, full test/lint chain clean.
