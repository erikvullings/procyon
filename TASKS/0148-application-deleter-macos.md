# 0148 Application deleter (macOS)

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: cross-cutting
Depends on: 0059, 0061

## Context

Identified from a competitive feature scan against ForkLift (2026-08-19 product-page discussion).
ForkLift can uninstall a macOS app along with its scattered support files; plain drag-to-Trash on
macOS only removes the `.app` bundle and leaves preferences, caches, application-support data, and
launch agents behind.

This is a macOS-specific win — Windows already has a real uninstaller convention (Programs &
Features / MSI uninstall) and Linux has package-manager removal, so neither platform has the same
gap. Scope this as a macOS-only feature behind the existing capability-flag pattern
(`PlatformCapabilities`), not a cross-platform subsystem.

## Acceptance Criteria

- A new action (e.g. `core.uninstallApplication`), available only when the selected entry is a
  `.app` bundle on macOS (gated by a new `PlatformCapabilities::APPLICATION_UNINSTALL` bit or
  equivalent, following the existing capability-gating pattern used for Trash/Reveal/Open-With in
  `crates/fm-application/src/action.rs`).
- Related-file discovery scans well-known locations for files/folders whose name matches the app's
  bundle identifier (`CFBundleIdentifier` from `Info.plist`) or product name: `~/Library/
  Application Support/`, `~/Library/Caches/`, `~/Library/Preferences/` (`.plist` files), `~/
  Library/Saved Application State/`, `~/Library/LaunchAgents/` and `/Library/LaunchAgents/` (listed
  only — writing to `/Library` requires elevation, out of scope for this task), `~/Library/Logs/`.
- The user reviews a checklist of discovered related files (each with its path and size) **before**
  anything is deleted — matching, an explicit confirm-then-act flow, not a scan of an entire
  filesystem, silent auto-delete. Nothing outside the well-known locations above is ever touched.
- Deletion goes through the same Trash/Recycle-Bin-first path as every other delete in the app
  (0061's `TRASH` capability), not a permanent unlink — an accidental match should be recoverable.
- False-positive risk is handled conservatively: prefer under-matching (miss a stray file) over
  over-matching (catch an unrelated file that happens to share a name fragment) — match on exact
  bundle identifier where available, and require a whole-path-segment match rather than a substring
  match when falling back to product-name matching.
- Tests: bundle-identifier extraction from a fixture `Info.plist`, related-file discovery against a
  fixture directory tree (including a deliberate false-positive case that must NOT match), and the
  confirm-then-trash flow.

## Implementation Notes

- `Info.plist` parsing: macOS `.app` bundles are directories: `<AppName>.app/Contents/Info.plist`.
  A `plist` crate (already common in the Rust ecosystem) reads `CFBundleIdentifier` /
  `CFBundleName` without shelling out.
- This is a self-contained, additive feature — no changes to the operation engine's core
  copy/move/delete semantics, just a new discovery step that produces a list of `Location`s to feed
  into the existing delete-to-Trash path.
- Consider whether the review checklist reuses the existing multi-selection Properties/delete-
  confirmation UI patterns rather than inventing new dialog chrome.

## Agent Notes

- 2026-08-26 claude: Implemented end to end, backend first (TDD) then frontend.
  - **fm-platform**: new `PlatformCapabilities::APPLICATION_UNINSTALL` bit (`crates/fm-platform/src/capabilities.rs`),
    new `UninstallCandidate { path, size_bytes, removable }` / `ApplicationUninstallPlan
    { bundle_identifier, product_name, related_files }` types (`types.rs`), and a new
    `PlatformAdapter::plan_application_uninstall` trait method (default `Unsupported`, per the
    existing capability-gating convention).
  - **fm-platform-macos** (`crates/fm-platform-macos/src/uninstall.rs`, new module): `read_bundle_info`
    parses `Contents/Info.plist` for `CFBundleIdentifier`/`CFBundleName` (falling back to
    `CFBundleDisplayName`, then the bundle's file-stem) via the existing `plist` crate dependency -
    a missing/malformed plist degrades to an empty `BundleInfo` rather than an error.
    `discover_related_files` scans exactly the seven locations named in the acceptance criteria
    (`UninstallSearchRoots` is parameterized so tests point it at a fixture tree instead of the real
    `~/Library`/`/Library`); matching is exact-bundle-identifier-first, falling back to a
    case-insensitive **whole-filename-segment** match against the product name (never substring) -
    `/Library/LaunchAgents` matches are reported with `removable: false` and are never offered for
    deletion (elevation is out of scope, matching the task note). Directory sizes are summed
    recursively with `walkdir` (added as a macOS-only dependency), never following symlinks.
    9 new unit tests cover bundle-identifier/product-name extraction (including the missing-plist
    fallback), exact-identifier matching, the deliberate `WidgetHelper`-vs-`Widget` false-positive
    case, the whole-segment product-name fallback, the `/Library` non-removable flag, recursive
    directory sizing, and bundle-path validation.
  - **fm-application**: new `core.uninstallApplication` action (`action.rs`), capability-gated
    single-selection like `core.revealInSystemFileManager`; the "must actually be a `.app` bundle"
    check stays client-side (no backend "entry kind" predicate exists, same documented gap as
    `core.calculateFolderSize`'s directory check). New read-only `FileManagerService::
    discover_application_uninstall_candidates` (`service.rs`) dispatches straight to the platform
    adapter (like `core.open`/`core.revealInSystemFileManager`, not through the operation engine),
    mapping `PlatformError::NotFound` to `ApplicationError::NotFound` and everything else through
    the existing `map_platform_error`. 5 new service tests + 2 new action-registry gating tests
    (capability independence, both directions).
  - **fm-transport-dto** / **fm-server**: new `DiscoverApplicationUninstallCandidatesRequestDto`/
    `ResponseDto`/`ApplicationUninstallCandidateDto` (`application_uninstall.rs`, camelCase, 2 new
    round-trip tests) and `POST /api/v1/applications/uninstall/discover`
    (`routes/application_uninstall.rs`), reusing `require_within_roots` exactly like
    `calculate_folder_size`. OpenAPI exported and the Orval TS client regenerated (`pnpm run
    api:export && pnpm run api:generate`) - no manual edits to either generated artifact.
  - **Confirm-then-trash**: deliberately reuses the *existing* Trash operation path end to end - no
    new mutating service method or operation-engine change. The frontend's confirm callback calls
    `OperationsController.trash([bundleLocation, ...checkedRelatedLocations])`, the same call
    `core.trash` makes for an ordinary multi-selection.
  - **Frontend**: `discoverApplicationUninstallCandidates` added to `FileManagerClient` and all three
    adapters (mock/http/Tauri - a real `apps/fm-desktop/src-tauri` command was added for Tauri
    parity, since a feature reachable through only one host would violate spec rule 9). New
    `ApplicationUninstallDialog` (`features/operations/application-uninstall-dialog.ts`): one row per
    discovered candidate with its path and formatted size, a checkbox pre-checked for every
    removable candidate, and a locked/no-checkbox row with an "administrator access" note for
    non-removable (`/Library`) candidates; "Move to Trash" only renders when the host actually
    supports Trash. Wired via `core.uninstallApplication` in `global-keydown-handler.ts` (keyboard)
    **and** `action-command-controller.ts`'s `invokePaletteAction`/`invokeContextMenuAction` (command
    palette and right-click menu) - the latter two were not wired by the first implementation pass
    and were added afterward, since without them the context-menu entry `availability.ts` makes
    visible would silently no-op instead of running discovery (verified with 3 new
    `action-command-controller.test.ts` tests: palette dispatch, context-menu dispatch, and
    context-menu no-op when unavailable). `availability.ts` requires the sole selection's name to
    end in `.app`.
  - **Full confirm-then-trash flow test**: extended the existing app-shell integration test (task
    covers "confirm-then-trash flow" explicitly) to click "Move to Trash" after discovery and assert
    `startOperation` is called with `type: 'trash'` and `sources` containing both the bundle and the
    checked related file - not just that the dialog opens.
  - **Verification**: `cargo test -p fm-platform -p fm-platform-macos` (53/53 + 1 ignored, pre-existing),
    `cargo test -p fm-application --lib` (241/241), `cargo test -p fm-transport-dto` (108/108),
    `cargo test -p fm-server` (all green; one pre-existing timing-sensitive SSE test - unrelated to
    this change, reproduced flaky on its own in isolation too), `cargo build -p fm-desktop` +
    `cargo clippy -p fm-desktop --all-targets -- -D warnings` (clean). `cargo fmt --all --check` and
    `cargo clippy` clean on every touched crate. Frontend: `pnpm --dir frontend exec tsc --noEmit`
    clean, `pnpm --dir frontend exec vitest run` **116 files / 1424 tests, all passing** (whole
    suite, not just new files), `pnpm run lint` clean except four pre-existing `noDescendingSpecificity`
    CSS warnings unrelated to this task.
  - **Known limitation**: `core.uninstallApplication` has no default keyboard shortcut
    (`Vec::new()` in `action.rs`, matching how most context-menu-only actions are registered) - it's
    reachable via the right-click context menu and command palette, not a bare keypress. A shortcut
    can be added later without any other change if wanted.
- 2026-08-26 claude: Two user-reported follow-ups, addressed together.
  - **Scoped to genuinely installed applications**: `plan_application_uninstall`
    (`crates/fm-platform-macos/src/uninstall.rs`) previously accepted any `.app`-named bundle
    anywhere on disk. It now rejects (via a new `is_installed_application` check, `PlatformError::Io`
    with a readable message) any bundle that isn't under `/Applications` or `~/Applications` (at any
    depth, so `/Applications/Utilities/*.app` still counts) - mirroring how real macOS uninstallers
    (ForkLift, AppCleaner) scope themselves, and incidentally excluding `/System/Applications`
    entirely since it was never in the trusted-roots list. 5 new unit tests (2 for
    `is_installed_application` directly, 1 end-to-end via a real fixture bundle placed outside
    `/Applications`, verified against the existing "not a bundle" and existing discovery tests).
  - **Dock icon cleanup**: uninstalling an app previously left a stale Dock icon (if the user had
    one pinned) pointing at the now-trashed bundle. New module
    `crates/fm-platform-macos/src/dock.rs`: `remove_dock_icon` reads
    `~/Library/Preferences/com.apple.dock.plist`, removes the one `persistent-apps` entry whose
    `tile-data.file-data._CFURLString` matches the bundle's `file://` URI (built via
    `fm_domain::Location::from_native_path`, comparing with the trailing slash macOS adds for
    directories normalized away), writes the plist back (`to_file_binary`), and restarts the Dock
    (`killall Dock`, best-effort, non-fatal) so the change is visible immediately. Exposed as a new
    `PlatformAdapter::remove_application_dock_icon` method (default `Unsupported`, reusing
    `APPLICATION_UNINSTALL` rather than a new capability bit), a new
    `FileManagerService::remove_application_dock_icon` (`Unsupported` degrades to `removed: false`,
    not an error - this is best-effort bookkeeping, never a required step), a new
    `POST /api/v1/applications/uninstall/remove-dock-icon` route, and a matching Tauri command for
    host parity. The frontend calls it fire-and-forget alongside (not gating) the existing
    `startOperation` Trash call in `app-dialogs.ts`'s confirm handler, so a failure here never blocks
    or delays the actual uninstall. 5 new `dock.rs` unit tests (pure `plist::Value` fixtures, no real
    Dock preferences file touched) + 3 new service-level tests (found-and-removed, unsupported
    degrades silently, a genuine I/O failure still surfaces) + 4 new frontend client tests (http x2,
    tauri x1, mock x1) + extended the existing confirm-then-trash `app-shell.test.ts` integration
    test to also assert `removeApplicationDockIcon` was called with the bundle location.
  - **Verification**: `cargo test -p fm-platform -p fm-platform-macos` (all green, 12 new uninstall
    tests + 5 new dock tests), `cargo test -p fm-application --lib` (259/259), `cargo test -p
    fm-transport-dto` (110/110), full `cargo clippy --workspace --all-targets -- -D warnings` and
    `cargo fmt --all --check` clean (one `needless_borrows_for_generic_args` caught and fixed).
    Frontend: `tsc --noEmit` clean, `vitest run` **116 files / 1428 tests, all passing**, `biome
    check` clean (auto-fixed one long-line format and two import-sort issues; same 3 pre-existing
    unrelated CSS warnings as before).
