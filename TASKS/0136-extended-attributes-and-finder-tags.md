# 0136 Extended attributes, Finder tags and Spotlight comments editor

Status: done
Priority: medium
Owner: unassigned
Agent: claude
Area: platform
Depends on: 0058, 0059

## Context

0059's Agent Notes explicitly deferred this: "Finder tags, extended attributes and drag-to-Finder
are declared as out of scope" for the initial macOS integration. Nobody picked it back up since.
Finder tags (colored labels) are one of the more commonly used macOS organizational features, and
reading/writing them (plus generic extended attributes / Spotlight comments) would let fm interop
properly with files a user has already tagged in Finder, rather than only seeing fm's own metadata.

## Acceptance Criteria
- Read Finder tags (`com.apple.metadata:_kMDItemUserTags` xattr) for entries in a directory listing
  and surface them as a column/badge, reusing the existing entry-icon overlay pattern from 0091
  where practical.
- Write/edit Finder tags from fm (assign an existing tag, remove a tag, create a new named tag with
  a color) — round-trips correctly with Finder (a tag set in fm is visible in Finder and vice
  versa).
- Read/edit the Spotlight comment (`kMDItemFinderComment`) for a single entry, surfaced via the
  properties/get-info surface if one exists by the time this is picked up (see 0129's Alt+Enter row
  — no properties dialog exists yet as of this writing; either build a minimal one here or land
  after that dialog exists).
- Windows/Linux: report `extendedAttributes`/`finderTags` capability as `false` rather than
  half-implementing an equivalent — this is explicitly a macOS-first feature (NTFS alternate data
  streams and Linux xattrs are different enough conventions that a shared UI abstraction should wait
  for a second concrete use case).
- Tests: xattr read/write round-trip (macOS-gated), tag color mapping, and capability-reporting
  false on non-macOS platforms.

## Implementation Notes
- Extends `fm-platform-macos`'s existing xattr-adjacent code paths (check what 0059/0091 already
  touch for icon overlays before adding a second xattr-reading code path).
- Finder tag color IDs and the user-tags plist format are undocumented-but-stable; base the
  implementation on existing open-source references (e.g. how `mdls`/`xattr` CLI tools decode them)
  rather than reverse-engineering from scratch.

## Agent Notes
- 2026-08-17 claude: Implemented end-to-end.
  - **`fm-platform`**: `FinderTag`/`FinderTagColor` domain types (color index mapping documented
    against the long-stable `tag` CLI convention: 0 none, 1 gray, 2 green, 3 purple, 4 blue,
    5 yellow, 6 red, 7 orange), `EXTENDED_ATTRIBUTES`/`FINDER_TAGS` capability bits, and 4 new
    default-unsupported `PlatformAdapter` methods (`finder_tags`/`set_finder_tags`/
    `spotlight_comment`/`set_spotlight_comment`).
  - **`fm-platform-macos`**: real implementation using the `xattr` + `plist` crates (added as new
    macOS-only target deps) rather than AppKit — reads/writes `com.apple.metadata:_kMDItemUserTags`
    (binary-plist array of `"name"` or `"name\ncolorDigit"` strings) and
    `com.apple.metadata:kMDItemFinderComment` (binary-plist string) directly. Setting an empty tag
    list or clearing a comment removes the xattr entirely (matches what Finder itself does), and
    removing an already-absent attribute is a no-op success, not an error. Windows/fallback report
    both capabilities `false` (no code touches them at all, per the acceptance criteria's explicit
    "don't half-implement an NTFS equivalent" instruction).
  - **Reading is a lazy per-entry fetch, not embedded in the directory listing**: acceptance
    criteria said "reuse the entry-icon overlay pattern from 0091", and unlike native icons (shared
    per extension) Finder tags are unique per file, so the closer real precedent is
    `ThumbnailLoader` (task 0134, keyed per entry uri) — `FinderTagsLoader` mirrors it exactly:
    lazy, deduped, cached, graceful fallback to "no badge" on any error/unsupported/loading. This
    avoids threading xattr reads through `fm-vfs-local`/`DirectorySnapshot` (which would need a new
    field piped through `fm-domain`→`fm-transport-dto`→frontend for every listing, most of it never
    rendered) and avoids the `fm-vfs-local`/`fm-platform-macos` same-layer dependency the crate
    layering fitness test forbids.
  - **`fm-transport-dto`**: `FinderTagColorDto`/`FinderTagDto`/`FinderTagsDto`/`SpotlightCommentDto`
    (new `finder_tags.rs` module); `FinderTagsDto`/`SpotlightCommentDto` are each used for both the
    GET response and the PUT request/response (mirrors `SettingsDto`'s get/put symmetry) rather than
    a separate request-wrapper type. `RuntimeCapabilitiesDto` gained `extendedAttributes`/
    `finderTags` booleans.
  - **`fm-application`**: `FileManagerService::finder_tags`/`set_finder_tags`/`spotlight_comment`/
    `set_spotlight_comment`, sharing a new `native_path_for` helper with `file_icon` (uri → native
    path). Two new action-registry entries, `core.editFinderTags`/`core.editSpotlightComment`
    (`fileOperations` category, no default shortcut, `capability_gated_single_selection`) — these
    are frontend-intercepted-only (like `core.createDirectory`), never dispatched through
    `invoke_action`'s operation engine. Error mapping: reads treat `Unsupported`/`NotFound` both as
    a graceful `ApplicationError::NotFound` (404, "no tags/no comment"); writes only special-case
    `NotFound` that way and surface everything else (including `Unsupported`, which the frontend
    should never trigger since it gates on capability first) as a real 502, matching
    `map_native_menu_error`'s established reasoning.
  - **`fm-server`**: `GET`/`PUT /api/v1/finder-tags` and `GET`/`PUT /api/v1/spotlight-comment`
    (`apps/fm-server/src/routes/extended_attributes.rs`), each registered as its own
    `utoipa_axum::routes!()` call (matches the existing settings GET/PUT precedent — a single call
    with multiple handlers is not how this codebase merges same-path methods).
  - **`fm-desktop`**: matching `get_finder_tags`/`set_finder_tags`/`get_spotlight_comment`/
    `set_spotlight_comment` Tauri commands, registered in both `invoke_handler!` lists (real app +
    the mock-IPC test builder).
  - **Frontend**: `FileManagerClient` gained the 4 methods (implemented on all three adapters, http
    signature confirmed against the real Orval-generated `(bodyDto, params, options?)` shape after
    regenerating — body first, matching every other query+body PUT in the generated client, not
    guessed). `FinderTagsLoader` (new, mirrors `ThumbnailLoader`) plus a small colored-dot badge in
    the directory table's name cell (`finder-tag-colors.ts` for the color→CSS-swatch mapping,
    approximate hex values, not pixel-matched to a macOS version). Two new minimal `ModalPanel`
    dialogs (`features/entry-metadata/finder-tags-dialog.ts` — chip list + color-swatch picker +
    add/remove, all-at-once edit matching Finder's own tag editor and `set_finder_tags`'s
    semantics; `spotlight-comment-dialog.ts` — plain textarea), reachable via the selection context
    menu's new "Edit Tags…"/"Edit Comment…" entries (`invokeContextMenuAction` and
    `invokePaletteAction` both route through one new shared `openEntryMetadataDialog` helper in
    `action-command-controller.ts`, so command-palette invocation works identically, not just the
    context menu). Not bound to Alt+Enter or folded into the 0140 properties dialog that landed on
    `main` mid-task — that dialog's own Implementation Notes explicitly anticipated this
    ("this task can ship without it and 0136 can extend the dialog later"); surfacing Finder
    tags/comment inside `PropertiesDialog` instead of/alongside these standalone dialogs is a
    reasonable follow-up, not attempted here.
  - **Known gap**: no automated GUI-level check that a tag written via fm is actually visible in a
    live Finder window (would need OS UI automation this environment doesn't have access to) — the
    round-trip is instead verified via `fm-platform-macos`'s tests independently re-reading the raw
    xattr bytes through the system `/usr/bin/xattr -p` CLI (a completely different code path from
    this crate's own `xattr`/`plist` decode) and asserting a genuine `bplist00`-prefixed binary
    plist landed under the exact name Finder reads.
  - **Verified**: `cargo test -p fm-platform` (10/10), `cargo test -p fm-platform-macos` (43/43,
    1 pre-existing ignored - opens a real Finder window), `cargo test -p fm-platform-windows`
    (capability-false test included), `cargo test -p fm-transport-dto` (103/103), `cargo test
    -p fm-application` (205/205 unit; one *unrelated*, pre-existing, load-induced timing flake in
    `conflict_resolution.rs`, reproduced independent of this task under this machine's heavy
    concurrent-build load), `cargo test -p fm-server --test extended_attributes_routes` (5/5),
    `cargo test -p fm-desktop` (21/21, incl. 2 new invalid-location IPC tests). Full workspace:
    `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D
    warnings`, `cargo fmt --all --check` all clean (zero warnings/errors). Frontend: `tsc --noEmit`
    clean, `pnpm exec vitest run` on the touched test files (66/66), `biome check .` clean (one
    pre-existing, unrelated CSS specificity warning at `theme.css:204`, predates this task).
    `pnpm run api:check`-equivalent (export + generate) run manually; regenerated
    `frontend/openapi/openapi.json` and `frontend/src/api/generated/**` committed.
  - Merged `main` mid-task (it had moved ~14 commits, including 0135/0140/0144 landing) per explicit
    user instruction; resolved 4 real conflicts (import-list conflicts in
    `fm-desktop/commands.rs`, `platform_mapping.rs`, `service.rs`, and `app-dialogs.ts` — all
    "both sides added things nearby," none semantic) and reran the full verification above
    afterward.
