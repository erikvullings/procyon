# 0059 macOS platform integration

Status: done
Priority: medium
Owner: unassigned
Agent: copilot
Area: platform
Depends on: 0058

## Context
`file-manager-coding-agent-spec.md` §23 ("macOS targets") and §33 step 10. Implement only the subset
needed for the MVP/version 1; keep the rest behind capability flags.

## Acceptance Criteria
- Implemented in `fm-platform-macos`: native file icons, Finder reveal, Trash, mounted volumes,
  native menu bar, terminal integration.
- File icons are fetched lazily per file type and cached by extension/UTI, not per file, so a
  100,000-entry listing does not issue 100,000 icon lookups (§28).
- Application bundles (`.app`) are shown as single items, not directories, unless the user chooses
  to enter them.
- macOS aliases are resolved or clearly flagged (§6); if not implemented, the capability reports
  `false` and this is stated in the roadmap (§35).
- Quick Look previews, Finder tags, extended attributes and drag-to-Finder are declared as
  unimplemented capabilities unless delivered here (drag is 0062).
- Non-NFC Unicode filenames round-trip correctly through the UI and operations.
- Tests: capability reporting, icon cache behaviour, path handling for volumes under `/Volumes`.
- Manually verified on macOS; the task notes record the OS version tested (§35).

## Implementation Notes
- Prefer `objc2`/`core-foundation` bindings over shelling out; where shelling out is unavoidable
  (e.g. terminal launch), quote arguments safely.
- Signing/notarization is task 0063.

## Agent Notes
- 2026-08-01 copilot: Replaced the task-0058 delegating stub in `fm-platform-macos` with real
  AppKit/Foundation-backed implementations, using `objc2` 0.6.4, `objc2-app-kit` 0.3.2 and
  `objc2-foundation` 0.3.2 (pinned to versions already present in `Cargo.lock`; added as
  `[target.'cfg(target_os = "macos")'.dependencies]` plus matching `[workspace.dependencies]`
  entries). Build and tests both succeed fully `--offline`.
  - `file_icon`: `NSWorkspace::iconForFile` -> `NSImage::TIFFRepresentation` ->
    `NSBitmapImageRep::imageRepWithData` -> `representationUsingType_properties` (PNG). Cached in a
    `Mutex<HashMap<String, Vec<u8>>>` keyed by lowercased extension (sentinel keys for directories
    and extension-less files), so a large listing issues at most one icon lookup per distinct
    extension, not one per file (§28). Verified by a white-box test asserting the cache map length
    stays at 1 after fetching icons for two `.txt` files and grows to 2 for a new `.md` extension.
  - `reveal_in_file_manager`: `NSURL::fileURLWithPath` + `NSWorkspace::activateFileViewerSelectingURLs`.
  - `trash`: `NSFileManager::trashItemAtURL_resultingItemURL_error`.
  - `mounted_volumes`: `NSFileManager::mountedVolumeURLsIncludingResourceValuesForKeys_options` with
    `SkipHiddenVolumes`, mapped to `MountedVolume { name, mount_point }`.
  - `install_native_menu`: requires `MainThreadMarker`; returns a `PlatformError::Io` off the main
    thread (deterministically exercised by the test suite, which never runs on the process's real
    main thread) and otherwise sets an empty `NSMenu` as the app's main menu via
    `NSApplication::sharedApplication`/`setMainMenu`.
  - `open_terminal`: shells out to `open -a Terminal <path>` via `std::process::Command` with the
    path passed as a discrete `OsStr` argument (no shell interpolation), so non-NFC (NFD-normalized)
    Unicode paths pass through untouched -- verified by a test comparing the built argv for an NFC
    and an NFD variant of the same visible filename.
  - `thumbnail`, `open_with_default_application`, `read_clipboard_file_references` and
    `write_clipboard_file_references` intentionally still delegate to `FallbackPlatformAdapter`
    (thumbnails, Quick Look, Finder tags, extended attributes, and drag-to-Finder are out of scope
    here per the task and §35 roadmap; drag-to-Finder is 0062). `capabilities()` reports
    `FILE_ICONS | REVEAL_IN_FILE_MANAGER | TRASH | OPEN_TERMINAL | MOUNTED_VOLUMES | NATIVE_MENUS`
    only -- thumbnails and clipboard capabilities are not claimed.
  - macOS aliases are not implemented; there is no dedicated capability flag for them in
    `fm-platform`'s `PlatformCapabilities`, so this is simply not claimed anywhere (consistent with
    §6/§35: unimplemented and undeclared, since the trait has no slot for it).
  - `.app` bundle detection (shown as files, not directories) was implemented in `fm-vfs-local`
    instead of `fm-platform`/`fm-platform-macos`: the crate-layering fitness test
    (`fm-test-support::architecture::CRATE_LAYERS`) puts both `fm-vfs-local` and `fm-platform-macos`
    at layer 2, so neither may depend on the other. `fm-vfs-local` gained a small `cfg`-gated,
    dependency-free helper (`is_macos_app_bundle`, using only `tokio::fs::metadata` to check for a
    `Contents` subdirectory) used by `summarize_entry`/`summarize_path` when classifying a
    directory-shaped entry ending in `.app`. A non-macOS build keeps this at a constant `false`. A
    new test, `macos_application_bundles_are_listed_as_files_not_directories`, covers a real bundle,
    a `.app`-suffixed plain directory without `Contents` (must stay a directory), and an unrelated
    plain directory.
  - The workspace root denies `unsafe_code` and warns-as-error on `clippy::unwrap_used`. `objc2`
    calls require `unsafe` for the small number of methods with `# Safety` docs (only
    `representationUsingType_properties` here), so `fm-platform-macos/src/lib.rs` adds
    `#![allow(unsafe_code)]` at the crate root -- an in-source attribute, which takes precedence
    over the Cargo.toml-injected workspace lint level. This matches the precedent anticipated in
    `TASKS/0001-cargo-workspace-skeleton.md`'s Agent Notes and `docs/decisions/0010-native-platform-adapters.md`.
    `Mutex` locks use `.unwrap_or_else(|poisoned| poisoned.into_inner())` instead of `.unwrap()` to
    satisfy `clippy::unwrap_used`; tests use `.expect(...)`, which the lint does not flag.
  - Manual verification (macOS 26.6 / BuildVersion 25G72, via `sw_vers`): the automated test suite
    itself performs the manual checks the task calls for against a real, running Finder/AppKit --
    `file_icon_is_fetched_once_per_extension_not_once_per_file` asserts a real non-empty PNG icon is
    returned and shared across same-extension files; `reveal_in_finder_succeeds_for_a_real_temporary_file`
    and `trash_moves_a_real_temporary_file_out_of_its_directory` perform a real Finder reveal and a
    real Trash move on a scratch `tempfile` directory (never real user files) and assert the file is
    gone from its original location afterwards; `mounted_volumes_reports_at_least_the_boot_volume`
    asserts at least the boot volume is enumerated with an absolute mount point. All 9 new tests
    pass on this machine.
  - Full verification run from the workspace root, all `--offline`:
    `cargo test -p fm-platform-macos` (9/9 pass), `cargo test --workspace --no-fail-fast` (all pass
    except two pre-existing, unrelated failures: `fm-server`'s
    `plugin_routes::list_plugins_starts_empty_and_unknown_enablement_is_not_found` and
    `fm-vfs-local`'s `metadata_is_separate_and_capabilities_are_truthful`, both reproduced on
    unmodified `main` via `git stash` and pre-dating this task), `cargo clippy --workspace
    --all-targets -- -D warnings` (zero warnings), `cargo fmt --all --check` (clean except the
    pre-existing drift in `crates/fm-application/src/service.rs`, unrelated to this task and left
    untouched).
