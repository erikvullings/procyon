# 0095 Distributable icon theme plugins

Status: done
Priority: medium
Owner: unassigned
Agent: Claude Sonnet 5 (Copilot)
Area: plugins
Depends on: 0053, 0085, 0092

## Context
User feedback on 0092 (Catppuccin icon theme): only a handful of extensions have Catppuccin icons
(Office documents, PDF, zip/rar/cbr/cbz, dmg, and many others are missing), and — the real
complaint — a theme can currently only be added by writing TypeScript in
`frontend/src/themes/*.ts` and getting it merged into this repo. That is not how icon theming
works in e.g. VS Code: there, a theme is a declarative JSON file (`iconDefinitions` plus
extension/filename mappings) shipped inside an extension package, installed and loaded at
runtime, with **no code and no upstream PR required**.

0085 already delivered a themeable *resolution registry* (`entryIconRegistry` in
`frontend/src/features/directory-table/entry-icons.ts`) — the extension point is real, but the
only two "installers" that exist (`createDefaultEntryIconRegistry()` and
`installCatppuccinIconTheme()`) are both hardcoded TypeScript compiled into the frontend bundle.
0092's `Settings.iconTheme` is a closed two-value enum (`generic | catppuccin`) for the same
reason. This task does not redesign the registry; it adds a third, data-driven way to populate it,
reusing the plugin discovery/permission/enablement infrastructure from 0053/0054/0057 instead of
inventing a new distribution mechanism.

Icon themes are a strictly easier case than action/column plugins: they need no code execution at
all, just static JSON + SVG assets, so they carry none of the Lua sandboxing concerns — only an
SVG-sanitization concern (see below).

## Design
- New manifest contribution, `[contributions] icon_theme = true`, alongside the existing
  `actions`/`columns` in `fm-plugin-api::PluginContributions`. A plugin declaring `icon_theme`
  needs no `entrypoint` executed (icon-theme plugins may omit Lua entirely — confirm whether
  `PluginManifest::entrypoint` should become optional or icon-theme plugins keep a no-op
  placeholder; prefer making it optional since requiring a fake Lua file is worse ergonomics for
  theme authors).
- A sibling file next to `plugin.toml`, e.g. `icon-theme.json`, schema modeled on VS Code's file
  icon theme format but trimmed to what `EntryIconRegistry` resolves:
  - `iconDefinitions: Record<string, { iconPath: string }>` — `iconPath` relative to the manifest
    directory, must resolve inside it (reject `..`/absolute paths).
  - `folder`, `folderExpanded` (reserved, no expand state in this app yet — accept but unused, or
    omit from v1), `file`, `symlink` — default definition keys.
  - `fileExtensions: Record<string, string>` — extension (no leading dot, lowercased) to
    definition key, feeding `entryIconRegistry.extensionIcons`.
  - `mimePrefixes: Record<string, string>` — feeding `entryIconRegistry.mimePrefixIcons` (not part
    of VS Code's format, needed because this registry has a MIME-prefix fallback tier).
- Backend (`fm-plugin-runtime`/`fm-application`/`fm-server`+Tauri command):
  - `discover_plugins` validates `icon-theme.json` when the contribution is declared (parse
    errors surface the same "invalid plugin, disabled with diagnostic" path 0053 already defined
    for manifests).
  - A new read-only route serves the theme's JSON and each referenced SVG asset, scoped strictly
    to that plugin's own directory (reuse/extend whatever path-containment check
    `fm-vfs-local`/plugin loading already does — do not add a new arbitrary-file-read surface).
    Needs both an HTTP route (`fm-server`) and a Tauri command, per `AGENTS.md` host parity.
  - `PluginDescriptorDto` gains enough info for the frontend to know a plugin offers an icon
    theme (id + display name), so Settings can list it.
- Frontend:
  - `Settings.iconTheme` changes from the closed `'generic' | 'catppuccin'` union to a plugin id
    string (`'generic'` stays the reserved built-in default, matching existing `IconTheme::Generic`
    on the Rust side). Settings Editor's Select is populated from discovered icon-theme plugins
    instead of a hardcoded two-item list.
  - A generic loader (replacing `installCatppuccinIconTheme`'s bespoke code) fetches
    `icon-theme.json` + referenced SVGs for the active theme id, parses the mapping, and installs
    it into `entryIconRegistry` — same registry, same install/restore contract 0085 already
    specifies (theme swap must still cleanly restore defaults per `restoreDefaultIconTheme`'s
    existing removal-of-theme-only-extensions behaviour).
  - **Security**: SVG markup now comes from a third-party plugin directory, not a vendored
    build-time constant — `m.trust()` on it as-is (the current `trustedIcon()` pattern) would be
    an XSS vector. Parse/sanitize before trusting: strip `<script>`, event-handler attributes
    (`on*`), and `<foreignObject>`; allow-list `svg`/`path`/`g`/`circle`/`rect`/`polygon` and their
    presentation attributes. Do this once per icon at theme-install time, not per render.
- Migrate the existing Catppuccin theme from `frontend/src/themes/catppuccin-icons.ts` into a real
  plugin package under `plugins/` (`icon-theme.json` + vendored SVGs, keeping the existing
  MIT-license attribution), proving the new mechanism replaces the hardcoded one rather than
  living alongside it indefinitely. Keep the Mocha-palette colors baked into each SVG's own
  `stroke`/`fill` attributes (no CSS var indirection needed — matches 0092's existing approach).
- Out of scope for this task: expanding icon *coverage* (Office/PDF variants, rar/cbr/cbz/dmg,
  etc.) — once this mechanism exists, coverage becomes a matter of shipping/updating theme
  plugins (including a fuller default `generic` set), not a core-repo code change. A follow-up
  can add those icons as plugin content once the loader exists.

## Acceptance Criteria
- `fm-plugin-api::PluginContributions` gains `icon_theme: bool`; manifest validation rejects an
  `icon_theme` contribution whose `icon-theme.json` is missing/malformed or whose `iconPath`
  entries escape the plugin directory.
- `discover_plugins` surfaces icon-theme plugins the same way it does action/column plugins
  (valid → listed enabled/disabled; invalid → listed disabled with a diagnostic, no startup
  failure).
- New HTTP route + Tauri command serve an icon-theme plugin's JSON manifest and SVG assets,
  read-only, path-contained to the plugin's own directory; covered by a route/command test
  including a path-traversal rejection case.
- Frontend has a generic icon-theme loader (no per-theme bespoke TypeScript) that installs any
  discovered icon-theme plugin into `entryIconRegistry`, with SVG sanitization applied before any
  `m.trust()`/equivalent, and a matching restore-to-default path.
- `Settings.iconTheme` (`IconTheme`/`IconThemeDto` on the Rust side too) becomes an open plugin-id
  string instead of the closed two-value enum; `'generic'` remains the reserved built-in default
  requiring no plugin lookup. OpenAPI/Orval regenerated (`pnpm run api:check` clean).
  Settings Editor's theme picker is populated from discovered icon-theme plugins.
- The Catppuccin theme ships as a real plugin under `plugins/` using the new mechanism;
  `frontend/src/themes/catppuccin-icons.ts` and its hardcoded install/restore functions are
  removed (or reduced to the shared sanitizer/loader utility, if factored out there).
- `docs/architecture/theming.md`'s "Catppuccin icon theme" subsection is replaced with a
  "Distributable icon theme plugins" subsection documenting the manifest schema, the security
  model (sanitization), and a worked example of a third-party theme package — this is the
  documentation a contributor needs to add a theme *without* touching this repo's source.
- Tests: manifest/schema validation (valid + several invalid cases incl. path traversal), the
  route/command tests above, frontend loader tests (install/restore/parse-error/sanitize
  behaviour, including a hostile-SVG-with-`<script>` fixture proving it's stripped), and an
  updated `settings-editor.test.ts`/`settings-model.test.ts` for the new `iconTheme` shape.

## Implementation Notes
- Reuse 0053's existing "invalid plugin → disabled with diagnostic, never fail startup" pattern
  and 0057's plugin management UI for surfacing icon-theme plugins alongside action/column
  plugins — don't build a parallel plugin list.
- Confirm whether `PluginManifest.entrypoint` needs to become `Option<PathBuf>` for icon-theme-only
  plugins (no Lua to run) before touching `fm-plugin-api`; this affects `ManifestError::InvalidField
  ("entrypoint")` validation and every existing manifest fixture/test.
- Look at how `fm-vfs-local`/existing plugin loading already contain paths within a root directory
  before writing a new containment check — don't reinvent that logic.

## Agent Notes
- Claude Sonnet 5 (Copilot): Implemented end-to-end, across backend and frontend.
  - **`fm-plugin-api`**: `PluginContributions.icon_theme: bool` (serde default false);
    `PluginManifest.entrypoint` changed to `Option<PathBuf>` (required only when
    `contributions.actions || contributions.columns`, so icon-theme-only plugins need no Lua at
    all). New `IconThemeManifest` (camelCase JSON, `icon-theme.json`) with `icon_definitions:
    BTreeMap<String, IconDefinition>`, optional `file`/`folder`/`symlink`, `file_extensions`/
    `mime_prefixes` maps. `validate()`/`parse()` reject empty `iconDefinitions`, unsafe
    (`..`/absolute) `iconPath`s, and any default/mapping value referencing an undeclared
    definition key.
  - **`fm-plugin-runtime`**: `discover_plugins` loads and validates `icon-theme.json` whenever
    `icon_theme` is declared, using the existing "invalid → disabled with diagnostic, never fail
    startup" pattern; icon asset paths are resolved through the same containment check used for
    other plugin assets. New tests cover a valid no-entrypoint icon-theme plugin, an unknown
    definition reference, an icon path escaping the plugin directory, and (added at the very end)
    `discovers_the_real_catppuccin_icons_plugin_package`, which discovers the real
    `plugins/catppuccin-icons/` package from disk and asserts its manifest/icon-theme shape.
  - **`fm-application`/`fm-server`/Tauri**: `Service::plugin_icon_theme_asset(plugin_id, path)`
    added, reused by both a new `GET /api/v1/plugins/{pluginId}/icon-theme/asset?path=...` route
    (`apps/fm-server/src/routes/plugin.rs`) and a Tauri `get_plugin_icon_theme_asset` command
    (`apps/fm-desktop/src-tauri/src/commands.rs`), both rejecting any path that isn't one of the
    theme's declared icon paths. `PluginDescriptorDto` gained an `icon_theme` field so the
    frontend can list icon-theme-capable plugins without a second discovery mechanism.
  - **`fm-settings`/`fm-transport-dto`**: `Settings.icon_theme` (and `SettingsDto.icon_theme`)
    changed from the closed `IconTheme` enum to a plain `String` (open plugin-id, `"generic"`
    remains the reserved built-in default requiring no plugin lookup). OpenAPI/Orval regenerated
    (`pnpm run api:export && pnpm run api:generate`); the now-orphaned generated
    `iconThemeDto.ts` was removed manually since Orval does not prune stale model files on its
    own.
  - **Frontend domain/client**: `Settings.iconTheme: string` in `frontend/src/models/settings.ts`;
    `PluginIconDefinition`/`PluginIconTheme`/`PluginDescriptor.iconTheme?` in
    `frontend/src/models/plugin.ts`; `FileManagerClient.getPluginIconThemeAsset()` implemented in
    all three adapters (HTTP, Tauri, mock).
  - **Frontend security**: `frontend/src/themes/svg-sanitizer.ts` (new) — an allow-list SVG
    sanitizer (`svg`/`path`/`g`/`circle`/`rect`/`polygon` elements plus presentation attributes
    only; strips `<script>`, `<foreignObject>`, and `on*` handlers) applied once per icon at
    theme-install time, before any `m.trust()`.
  - **Frontend loader**: `frontend/src/themes/plugin-icon-theme.ts` (new) —
    `installPluginIconTheme(client, pluginId, iconTheme)`, a generic, data-driven replacement for
    the old hardcoded `installCatppuccinIconTheme()`; re-exports `restoreDefaultIconTheme` from
    `entry-icons.ts` unchanged. `settings-editor.ts`'s icon-theme `Select` is now built from
    `plugins.filter(p => p.iconTheme !== undefined)`. `app-shell.ts`'s `applyAppearance()` (and the
    `listPlugins()` race-condition path) call the new loader instead of the old binary check.
  - **Migration**: `plugins/catppuccin-icons/` created as a real, distributable plugin package
    (`plugin.toml` with `contributions.icon_theme = true` and no `entrypoint`; `icon-theme.json`
    with 27 icon definitions; `icons/*.svg` vendored verbatim from the old hardcoded theme, MIT
    attribution preserved). `frontend/src/themes/catppuccin-icons.ts` and its test file were
    deleted; a stale doc-comment cross-reference in `frontend/src/components/tabler-icons.ts` was
    updated.
  - **Docs**: `docs/architecture/theming.md`'s "Catppuccin icon theme" subsection replaced with
    "Distributable icon theme plugins", covering the manifest schema, asset-serving route/command,
    the sanitization security model, the generic loader, and `plugins/catppuccin-icons/` as a
    worked example.
  - **Verification**: `cargo test --workspace --target-dir /tmp/fm-target-test` all green except
    one pre-existing, unrelated failure (see below); `cargo clippy -p fm-plugin-runtime -p
    fm-plugin-api --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean for every
    file touched this task (pre-existing diffs remain in unrelated files —
    `crates/fm-plugin-api/src/lib.rs`, and a couple of lines in `fm-plugin-runtime/src/lib.rs`
    predating this task — left untouched to keep the diff scoped). Frontend: `pnpm exec tsc
    --noEmit` clean; `pnpm exec vitest run` → 59 files / 453 tests passing. `biome check .` clean
    for every file touched this task (pre-existing diffs remain in `app-shell.ts`,
    `navigation.ts`, `navigation.test.ts` — the user's own separate in-progress Ctrl+Tab work,
    deliberately left untouched).
  - **Pre-existing, unrelated test failure noted, not fixed**: `apps/fm-server/tests/
    plugin_routes.rs`'s `list_plugins_starts_empty_and_unknown_enablement_is_not_found` asserts
    zero discovered plugins, but `FileManagerService`'s bundled plugin directory is compile-time
    baked to this repo's own `plugins/` (`service.rs`: `PluginDiscovery::new(...)
    .with_bundled_directory(CARGO_MANIFEST_DIR/../../plugins)`), which already contained two
    committed sample plugins before this task (confirmed via a before/after run with
    `plugins/catppuccin-icons/` temporarily removed: fails with `left: 2` even without this
    task's new plugin). Adding `plugins/catppuccin-icons/` only changes the failure's count from
    2 to 3 — it does not introduce a new failure. Out of scope for this task to fix (it's a stale
    test assumption from before any bundled plugins existed, unrelated to icon theming); flagging
    here per the repo's "report incomplete/platform-untested behaviour explicitly" convention.
  - **Not committed**: the working tree also contains the user's own separate, unrelated
    in-progress Ctrl+Tab tab-cycling changes (`app-shell.ts`, `navigation.ts`, `pane.ts`,
    `directory-table.css`, `theme.css`). Per operational safety rules, no commit was made — the
    user should review and commit this task's files explicitly (or ask for a scoped commit).
