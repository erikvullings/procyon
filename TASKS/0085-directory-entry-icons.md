# 0085 Directory entry icons

Status: done
Priority: medium
Owner: unassigned
Agent: Claude Sonnet 5 (Copilot)
Area: frontend
Depends on: 0058

## Context
Follow-up from a pane-regression investigation: the user expected per-entry file/folder icons in
the directory table, assuming task 0059 (macOS platform integration) had added them. It didn't —
0059/0060 only implemented native icon *fetching* as a backend capability
(`fm-platform-macos`/`fm-platform-windows` `file_icon`, cached by extension/UTI) with no HTTP
route, Tauri command, or frontend consumer. `EntrySummary.icon_key` is hardcoded to `None`
everywhere it's constructed (`fm-vfs-local`, `fm-search`, `fm-application/directory.rs`). The
runtime capability flag `nativeFileIcons` (`RuntimeCapabilitiesDto.native_file_icons`) already
flows to the frontend (see `crates/fm-application/src/service.rs` `runtime_capabilities()`) but is
currently unused. As a stopgap, generic kind-based glyphs (`folderIcon`/`fileIcon`/`symlinkIcon` in
`frontend/src/components/icons.ts`) were added to the `core.name` column in
`frontend/src/features/directory-table/directory-table.ts`; this task replaces/extends that.

## Design question to resolve first
Two ways to source icons, not mutually exclusive:
1. **Theme plumbing (frontend-only):** a themeable per-kind/per-extension icon map (SVG glyphs or
   a `mask-image`/CSS-custom-property based icon font), resolved entirely client-side, no backend
   involvement. Matches the existing `--fm-*` token pattern in `docs/architecture/theming.md`.
2. **Served from the background:** the backend fetches real OS icons (already implemented for
   macOS, task 0060 for Windows) and serves them as bytes over a new HTTP route + matching Tauri
   command, keyed by `icon_key`/extension; the frontend fetches and caches them.

**Recommendation:** do both, layered — (1) is the baseline and the actual "theme" surface (always
available, zero network/IPC cost, instantly swappable), and (2) is an opt-in enhancement that
overlays real native icons on top of it when `runtimeCapabilities.nativeFileIcons` is true and the
icon has loaded, falling back to (1) while loading/unavailable/on non-native hosts (browser mode,
or platforms without a `fm-platform-*` icon implementation). This keeps parity across hosts (per
`AGENTS.md`, browser and Tauri must both work) since browser mode simply never gets past the
theme-icon fallback unless `fm-server` also serves native icons.

## Acceptance Criteria
- A themeable icon-resolution module (e.g. `frontend/src/features/directory-table/entry-icons.ts`)
  maps `EntryKind` (`file`/`directory`/`symlink`) plus `extension`/`mimeType` to an icon renderer,
  replacing the current inline `entryTypeIcon` in `directory-table.ts`.
- **Theme-creator replaceability is a hard requirement**: the icon set must be overridable without
  editing `directory-table.ts` — e.g. a single exported map/registry keyed by extension/kind that a
  theme package can import and extend/replace, or CSS custom properties (`--fm-icon-*`) analogous
  to the existing `--fm-*` token contract in `docs/architecture/theming.md`. Document the extension
  point there.
- Default icon set ships built-in (folder/file/symlink at minimum; a handful of common extensions
  such as image/archive/audio/video/pdf is a reasonable v1 scope — do not attempt exhaustive
  extension coverage).
- When `runtimeCapabilities.nativeFileIcons` is true: a new backend endpoint (HTTP route in
  `fm-server` + Tauri command, both calling the existing `PlatformAdapter::file_icon`) serves icon
  bytes keyed by extension/UTI (not per-entry — preserve the existing one-lookup-per-extension
  cache behaviour from 0059/0060, §28). The frontend fetches lazily (on first row render of a
  given extension) and caches client-side (in-memory is enough; no need to persist across reloads).
- Native icon fetch failures or unsupported hosts silently fall back to the themed glyph — never a
  broken image or blank cell.
- Works identically in both `pnpm dev:http` (browser) and `pnpm dev:tauri` (desktop) hosts; a host
  without the capability (browser talking to a non-icon-serving `fm-server`, or `nativeFileIcons:
  false`) only ever shows the themed glyphs, which is an acceptable, fully-functional default.
- Tests: icon-resolution map unit tests, a directory-table render test asserting the right themed
  glyph per kind/extension, and (if the backend piece lands in this task rather than a split
  follow-up) a route/command test asserting the cache-per-extension behavior end-to-end.

## Implementation Notes
- `crates/fm-platform/src/adapter.rs`'s `PlatformAdapter::file_icon` already exists and is
  implemented for macOS (0059); Windows (0060) status should be checked before assuming both hosts
  have it — if only macOS does, ship the native-icon layer as capability-gated per §28/§35 roadmap
  conventions the same way other partial platform features are declared.
- Consider splitting this into two tasks (frontend theme-icon baseline vs. backend-served native
  icon overlay) if the combined scope proves too large for one PR — the baseline alone already
  satisfies the user's immediate "I can't see any icons" complaint and has no backend dependency.
- Reuse the `icon()` helper pattern in `frontend/src/components/icons.ts` for any new built-in SVG
  glyphs (`currentColor` fill, consistent `viewBox`, `.fm-icon` class) rather than inventing a new
  icon primitive.

## Agent Notes
- 2026-08-04 Codex: Re-verified and closed the frontend theme-icon baseline after confirming the
  native backend overlay remains explicitly tracked by task 0091. The shipped implementation in
  commit `79a1aa8` satisfies 0085's split-scope acceptance criteria: kind/extension/MIME
  resolution, built-in themed glyphs, a replaceable registry extension point, documented theming,
  and directory-table rendering. Verified the 8 tests introduced by that commit (7 in
  `entry-icons.test.ts`, 1 in `directory-table.test.ts`) by rerunning both files (32 current tests
  passing across them), the full frontend suite (467 passed, 1 skipped), `pnpm --dir frontend
  typecheck`, and `pnpm run lint:frontend`. No implementation changes were required. The HTTP
  route, Tauri command, native lazy-fetch/cache overlay, and binary response handling are not gaps
  in this closed split scope; they remain acceptance criteria of open follow-up task 0091.
- 2026-08-03 Claude Sonnet 5 (Copilot): Shipped the frontend theme-icon baseline only; split the
  backend-served native-icon overlay into a new follow-up task, 0091, per this task's own
  explicit split-scope allowance ("Consider splitting this into two tasks..."). Status stays
  `open` — see 0091 for the remaining acceptance criteria this task's own text describes
  (`nativeFileIcons`-gated backend route/command, lazy fetch/cache, overlay-with-fallback).
  - **Built**: `frontend/src/features/directory-table/entry-icons.ts` — a themeable icon
    resolution registry (`entryIconRegistry`, mutable `kindIcons`/`extensionIcons`/
    `mimePrefixIcons` maps + `createDefaultEntryIconRegistry()` for isolated test copies),
    resolving `EntryKind`/extension/MIME type to an icon renderer. Five new extension-badge SVG
    glyphs added to `frontend/src/components/icons.ts` (`imageIcon`, `archiveIcon`, `audioIcon`,
    `videoIcon`, `pdfIcon`), reusing the existing `icon()` helper and file-body path, covering
    image/archive/audio/video/pdf per the Acceptance Criteria's v1 scope.
  - `frontend/src/features/directory-table/directory-table.ts` now imports `entryIcon` from the
    new module instead of the removed inline `entryTypeIcon`; the `core.name` column renders it
    unchanged otherwise.
  - Documented the new `entryIconRegistry` extension point in
    `docs/architecture/theming.md` under a new "Directory entry icons" section (satisfies the
    hard theme-replaceability requirement: a theme package mutates the exported registry's maps,
    no edit to `directory-table.ts` needed).
  - **Tested**: `frontend/src/features/directory-table/entry-icons.test.ts` (7 new tests:
    kind resolution for directory/symlink, extension resolution incl. case-insensitivity, MIME
    prefix fallback, unknown-extension fallback to the generic file icon, and a registry-override
    test proving theme replaceability) + 1 new test in `directory-table.test.ts` asserting the
    correct `.fm-icon-*` class renders per kind/extension in an actual mounted row. Verified via
    `pnpm exec vitest run src/features/directory-table/entry-icons.test.ts
    src/features/directory-table/directory-table.test.ts` (30/30 passing in those two files) and
    then the full suite: 406/406 passing (up from a 398-test baseline, +8: the 7 new
    `entry-icons.test.ts` tests + 1 new `directory-table.test.ts` test). `pnpm exec tsc --noEmit`
    clean, `pnpm run lint:frontend` (biome) clean. No Rust code touched, so `cargo` checks were
    not re-run for this commit.
  - **Deferred to 0091** (not done, no code written for these): the HTTP route + Tauri command
    calling `PlatformAdapter::file_icon`, OpenAPI/Orval regeneration, the `getFileIcon` client
    method on all three adapters, and the lazy-fetch/cache/overlay wiring gated on
    `runtimeCapabilities.nativeFileIcons`. 0091 also flags a real pre-existing risk found while
    scoping this: `frontend/src/api/fetch-mutator.ts`'s `readBody()` decodes every non-JSON
    response via `.text()`, which would corrupt binary PNG bytes — needs a real fix as part of
    0091, not a per-call workaround.
- 2026-08-04 Claude Sonnet 5 (Copilot): Closing out this task as `done`. Re-verified the frontend
  baseline is intact and unchanged (`entry-icons.test.ts`, `directory-table.test.ts` — 43 tests
  across the three icon-theme test files, verified via `pnpm exec vitest run
  src/features/directory-table/entry-icons.test.ts src/features/directory-table/directory-table.test.ts
  src/themes/plugin-icon-theme.test.ts`, all passing; working tree clean). The hard
  "theme-creator replaceability" requirement this task set out has since been proven and exceeded
  by two follow-up tasks built directly on the `entryIconRegistry` extension point:
  - Task 0092 shipped a concrete, hardcoded Catppuccin (Mocha) icon theme installed into the same
    registry, plus a `Settings.iconTheme` field and a live-switching Select in the Settings Editor.
  - Task 0095 went further and replaced that hardcoded approach with a real **distributable icon
    theme plugin mechanism** (`[contributions] icon_theme = true` + a sibling `icon-theme.json`,
    served read-only over HTTP/Tauri, sanitized before `m.trust()`, installed via a generic
    `installPluginIconTheme()` loader) — i.e. a theme package can now be added with zero changes to
    this repo's source, which is a stronger proof of the replaceability requirement than this task
    itself demanded. `plugins/catppuccin-icons/` is the real, currently-installed example of this.
  - `docs/architecture/theming.md` documents both the original `entryIconRegistry` extension point
    ("Directory entry icons" section, from this task) and the plugin mechanism built on top of it
    ("Distributable icon theme plugins" section, from 0095) — no further doc changes needed here.
  The one line of this task's own Acceptance Criteria still unmet — the `nativeFileIcons`-gated
  backend-served overlay — remains intentionally out of scope for this task per its own
  explicit split allowance; it stays tracked solely under task 0091 (open, low priority,
  unstarted), which is not blocked by anything else in this task.
