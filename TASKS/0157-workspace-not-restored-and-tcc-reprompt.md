# 0157 Workspace/folders not restored on relaunch, and TCC access re-prompts (alpha 5)

Status: in_progress
Priority: high
Subsystem: backend, desktop
Depends on: none

## Context

User report (2026-08-26, packaged alpha 5 build): quitting and reopening the app does not restore
the last-open folders/ephemeral workspace, and the user has to re-grant macOS TCC (Files and
Folders) access again.

This is a regression against [TASKS/0143](0143-workspace-restore-and-multi-window-placement.md),
marked `done`, whose own Agent Notes flag **six** separate implementation passes on exactly this
restore-across-relaunch behavior (the "ephemeral workspace" redesign of 2026-08-21/22 in
particular) — and every single pass ends with some variant of "not yet confirmed in the real app;
no tool in this session can drive the native multi-window Tauri app." This is very likely the first
time this behavior has actually been exercised end-to-end by a real user in a real packaged build.

Two distinct symptoms reported together — investigate whether they share a root cause before
assuming they're independent:

1. **Last-open folders/ephemeral workspace not restored.** Per 0143's design (2026-08-21/22 notes):
   a named workspace is an immutable template; every window gets a private "ephemeral" workspace
   forked from a template, persisted-but-only-synced-back-explicitly (`resync`/"Sync Workspace").
   Phase 2 of that redesign claims closing an individual window deletes its own ephemeral workspace,
   but quitting the *whole app* should NOT delete ephemeral workspaces (a `QuittingFlag` is meant to
   distinguish the two), and `apps/fm-desktop/src-tauri/src/lib.rs`'s `setup()` is meant to rebuild
   one window per surviving ephemeral workspace on the next launch. **Prime suspect**: the
   `QuittingFlag`/`RunEvent::ExitRequested`/`Exit` detection is misfiring (treating a full quit as
   individual window closes), so ephemeral workspaces get deleted before `setup()` ever sees them —
   this would exactly match "did not restore my last open folders," since without an explicit
   "Sync Workspace" the user's navigation state only ever lived in the now-deleted ephemeral
   workspace, never in any named one. Read 0143's "Phase 1"/"Phase 2" Agent Notes entries in full
   before touching this code — they document the exact files and mechanism already.
   - Also consider: is this actually working-as-designed (ephemeral-by-default, explicit-sync-only)
     but simply not what the user wants/expects? If the code is behaving exactly as 0143 designed
     it, this may be a product decision to revisit (auto-persist ephemeral workspaces without
     requiring explicit sync) rather than a pure bug — surface that distinction clearly once
     root-caused, don't just patch code to match a design that was deliberately chosen.
2. **TCC (Files and Folders) access re-prompts every launch.** 0143's 2026-08-16 notes explicitly
   discuss a TCC "no permission" issue but frame it as a *dev-rebuild* artifact ("dev-binary
   rebuilds routinely invalidate [grants], not something introduced by this task's changes... user
   needs to re-grant it themselves, not actionable from here") — but the user here is running a
   packaged **alpha 5** release build, not a dev rebuild, and reports this within the same installed
   build across quit/relaunch, which the prior note's explanation does not cover. Investigate
   separately from that prior (dev-only) observation:
   - Check whether the app is sandboxed (App Sandbox entitlement) — if so, TCC/folder access for
     user-selected locations requires persisting security-scoped bookmarks
     (`startAccessingSecurityScopedResource`/`NSURL` bookmark data) and re-resolving them on next
     launch; a `grep` across the repo during initial triage found **zero** references to security
     scoping anywhere in `crates/` or `apps/` — if the app is sandboxed, this is very likely the
     direct cause and needs implementing from scratch.
   - If the app is NOT sandboxed, standard TCC grants are normally tied to (bundle id + code
     signature) and persist across relaunches of the same signed build without any bookmark
     handling — re-prompting every launch in that case would point instead at the release/signing
     pipeline (e.g. ad-hoc signing, or a signature identity that isn't stable across the same
     build), which is more of a packaging/build config issue than application code. Check
     `apps/fm-desktop/src-tauri/tauri.conf.json`'s `bundle.macOS` signing config and
     `.github/workflows/release-desktop.yml` before assuming it's a code bug.

## Acceptance Criteria
- Root cause of each symptom identified with evidence, not guessed — read 0143's existing notes
  first so this doesn't re-derive already-documented mechanism details.
- Ephemeral workspace / last-open folders survive an actual quit-and-relaunch of the packaged app
  (or, if this turns out to be working-as-designed rather than a bug, that's surfaced explicitly to
  the user as a design question, not silently left as "already correct").
- TCC access does not need to be re-granted across relaunches of the same signed build, OR (if this
  turns out to be a signing/packaging issue rather than app code) the actual cause is identified and
  reported clearly even if the fix lives outside this repo's application code (e.g. release
  workflow, entitlements, signing identity).
- If security-scoped bookmarks are the fix, they're persisted per-location the user has granted
  access to and re-resolved on next launch before any provider tries to read that location.

## Implementation Notes
- Start by reading every Agent Notes entry in [TASKS/0143](0143-workspace-restore-and-multi-window-placement.md)
  in full — six prior passes already traced most of the multi-window/ephemeral-workspace machinery
  in detail (file paths, exact bugs found and fixed, and what was never confirmed). Don't re-derive
  what's already documented there.
- Key files already identified by 0143's notes: `apps/fm-desktop/src-tauri/src/lib.rs` (`setup()`,
  `QuittingFlag`, `RunEvent` handling, `Destroyed` handler), `apps/fm-desktop/src-tauri/src/commands.rs`
  (`open_workspace_window`, `build_workspace_window`, `build_default_window`,
  `declared_window_config`), `crates/fm-application/src/workspace/service.rs`
  (`WorkspaceService::start`/`fork`/`resync`, `most_recently_updated_named_workspace_id`).
- For TCC/entitlements: check `apps/fm-desktop/src-tauri/Info.plist` or equivalent entitlements
  file (if one exists) for `com.apple.security.app-sandbox` and any
  `com.apple.security.files.*` keys, and `tauri.conf.json`'s `bundle.macOS.entitlements` field.
- This needs the real native app to confirm — no tool in this environment can drive/screenshot a
  Tauri desktop window (same standing limitation 0143 hit six times). Get as far as possible via
  code reading + `cargo test`/`cargo build` + reasoning about the Tauri/`RunEvent` lifecycle, then
  hand back to the user for an actual quit/relaunch test rather than claiming unverified success.

## Agent Notes
- 2026-08-26: Task created from a direct user bug report (packaged alpha 5 build) — regression
  against six previously-unconfirmed passes on TASKS/0143. Not yet investigated.
- 2026-08-26: Root-caused and fixed symptom 1 (workspace/folders not restored); found and applied a
  correction to symptom 2 (identity, not TCC-code-path) that needs the user's confirmation.
  - **Symptom 1 — root cause found, NOT the `QuittingFlag` misfire this task's Context guessed at.**
    On macOS, closing the app's only window (the ordinary red-button close) does not quit the
    process — standard Mac convention, and exactly what `QuittingFlag` already correctly
    distinguishes from a real quit. The actual bug: nothing in this codebase handled
    `tauri::RunEvent::Reopen` (macOS's `applicationShouldHandleReopen`, fired when the user clicks
    the Dock icon while the app has no visible windows). `tauri_plugin_single_instance`'s callback
    only fires for a genuinely *new* OS-level process launch (double-clicking the .app again in
    Finder) — it does not fire when macOS reactivates an already-running, windowless process via the
    Dock icon. So: user closes their one window (correctly deletes its own ephemeral workspace, per
    0143's existing by-design behavior — not a bug) → app keeps running invisibly with zero windows
    → user clicks the Dock icon expecting to "reopen" → nothing happens, silently, since no code path
    creates a window for that case → user likely force-quits and relaunches a truly fresh process →
    by then the ephemeral workspace is already gone. This matches "did not restore my last open
    folders" exactly, and doesn't require the `QuittingFlag`/`ExitRequested` ordering to be broken at
    all (traced through `tauri` 2.11.5's source: `ExitRequested`/`Exit` do fire before window
    `Destroyed` events on a real quit, as 0143's design intended — that part was already correct).
    - Fix: factored `lib.rs`'s `setup()` cold-start restore logic (list workspaces, find surviving
      ephemeral ones, build a window per surviving one or one default window) into a new shared
      `commands::open_startup_windows`, and added a `tauri::RunEvent::Reopen { has_visible_windows,
      .. }` handler (macOS-only) in `lib.rs`'s `.run()` closure that calls it via `tokio::spawn` when
      `has_visible_windows` is false. `setup()` itself now just calls the same function via
      `block_on` instead of duplicating the logic inline.
    - Did not find or need to touch anything about the "is this working as designed vs. is it a
      bug" question this task's Context raised — the ephemeral-workspace persistence logic was never
      actually reachable to test in the real app before now, and this fix makes it reachable via the
      Dock icon the way it was originally intended to work.
    - Verified: `cargo check -p fm-desktop --tests`, `cargo clippy -p fm-desktop --all-targets -- -D
      warnings`, `cargo fmt -p fm-desktop -- --check`, `cargo test -p fm-desktop --lib` (23/23,
      including the `app_starts_and_reports_the_tauri_runtime` boot smoke test) — all clean. **Not
      independently confirmed in the real packaged app** (same standing limitation every 0143 pass
      hit — no tool here can drive a native multi-window Tauri app). Ask the user to: close the
      window (not Cmd+Q), click the Dock icon, and confirm a window reopens with their prior
      state — that's the specific path this fix targets.
  - **Symptom 2 — user independently confirmed via `codesign -dv --verbose=4` that the installed
    app is unsigned** (`Signature=adhoc`, `TeamIdentifier=not set`) with an unstable identifier
    (`Identifier=fm_desktop-<hash>`, the hash suffix changes per build) — matching this task's
    "check signing config" note, not the security-scoped-bookmarks hypothesis (app is not
    sandboxed, so that mechanism doesn't apply here). Two concrete, low-risk fixes applied:
    1. `apps/fm-desktop/src-tauri/tauri.conf.json` and `Cargo.toml`'s
       `[package.metadata.desktop]` (the latter is the actual source of truth for release
       builds — read by `scripts/build-tauri.mjs` via `cargo metadata` and overlaid onto the Tauri
       config at build time; both needed the same values to avoid the drift the overlay
       architecture is specifically meant to prevent) now set `identifier =
       "nl.erikvullings.procyon"` instead of the placeholder `"dev.fm.desktop"`.
    2. Added `"mainBinaryName": "Procyon"` to `tauri.conf.json` (was never set — confirmed via
       `tauri-utils`' own doc comment that `productName` does *not* rename the compiled binary,
       only `mainBinaryName` does; this is why the executable was `fm-desktop` despite
       `product-name = "Procyon"` already being set in `Cargo.toml` before this session).
    - **This does not fully solve the underlying TCC-persistence problem by itself** — the app is
      still unsigned (ad-hoc), and a stable identifier is necessary but likely not sufficient for
      TCC grants to survive across arbitrary future rebuilds; real reliability needs a paid Apple
      Developer ID certificate and notarization in `release-desktop.yml`, which is a cost/business
      decision outside this task's scope, not something to silently implement. What this change
      does fix regardless: the identifier and binary name are now stable and brand-correct rather
      than a placeholder + a hash that changes every build.
    - **One-time transition cost, checked and found NOT to apply here**: changing the bundle
      identifier could in principle orphan existing users' local app data if that data were stored
      under an identifier-scoped path — checked `JsonFileWorkspaceRepository::default_directory()`
      (`crates/fm-application/src/workspace/persistent.rs`) and confirmed it uses
      `dirs::config_dir()/fm/workspaces` (a hardcoded `"fm"` folder, not the Tauri bundle
      identifier) — so this identifier change does **not** reset any user's saved workspaces/settings.
    - Also added a `binary` stanza to the generated Homebrew Cask
      (`scripts/generate-package-manager-files.mjs`) — `brew install --cask procyon` previously
      installed only the `.app` bundle with no `procyon` command on `PATH` (the user asked why `open
      -a Procyon`/`open /Applications/Procyon.app` were needed from Terminal; this was the answer).
      Now symlinks `Contents/MacOS/Procyon` (matching the new `mainBinaryName`) as `procyon`.
      Manually verified the script's output is syntactically correct generated Ruby.
    - **Not independently verified**: whether the identifier change alone measurably reduces
      TCC re-prompting in practice on the user's machine — ask them to confirm after installing a
      build with this change, understanding it's the unsigned/ad-hoc status (not fixed here) that's
      the more likely remaining cause of any continued re-prompting.
- 2026-08-30: A real relaunch still opened two fresh home tabs. The persisted store provided the
  missing evidence: older named workspaces still contained the user's tabs, but each failed launch
  had created another `Default N` workspace. `WorkspaceService::start` had two related holes:
  explicitly starting a restored ephemeral workspace wrote that ephemeral id into
  `last-active.json`, and a later startup whose last-active id no longer existed immediately
  created a fresh default instead of using the already-implemented most-recent named fallback.
  `start` now records an ephemeral workspace's named source as last-active (and never the ephemeral
  id itself), and a stale implicit selection falls back to the most recently updated named
  workspace before creating anything. Regression tests cover both paths.
