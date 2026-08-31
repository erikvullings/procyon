# 0092 Catppuccin icon theme

Status: done
Priority: low
Owner: unassigned
Agent: Claude Sonnet 5 (Copilot)
Area: frontend
Depends on: 0085

## Context
User request during an unrelated task pipeline: "Which directory icons are you considering? I
like https://github.com/catppuccin/vscode-icons" — followed by an explicit choice to build a
concrete Catppuccin-icons-based theme now, using the extension point delivered by task 0085
(`entryIconRegistry` in `frontend/src/features/directory-table/entry-icons.ts`).

A lightweight, `localStorage`-only preference was considered first for speed, but
`frontend/src/app/app-shell.ts` already documents (in a comment near `applyAppearance`) that
specification §26 keeps settings on the backend rather than in browser storage. This ruled out the
shortcut and mandated implementing the icon theme choice as a proper `Settings`/`SettingsDto`
field, following the exact pattern of the existing `theme`/`dateFormat`/`sizeFormat` fields.

## Acceptance Criteria
- A new `Settings.iconTheme: 'generic' | 'catppuccin'` field (default `'generic'`), mirrored by
  `IconTheme`/`IconThemeDto` on the Rust side, round-tripped through `fm-settings` persistence
  without a schema-version bump (relying on `#[serde(default)]` for old on-disk files).
- OpenAPI/Orval regenerated so the frontend's generated `SettingsDto`/`IconThemeDto` include the
  new field.
- A vendored Catppuccin (Mocha flavor) icon set covering folder/file/symlink kinds plus a curated
  set of common source-code extensions and MIME-prefix fallbacks, installed into the existing
  `EntryIconRegistry` extension point from 0085 — no changes to `directory-table.ts` needed.
- Installing/restoring the icon theme is driven by `Settings.iconTheme` and applied live from
  `app-shell.ts`'s `applyAppearance()`, matching how `theme`/`fontSize`/`rowHeight` are already
  applied (initial load, settings-editor live preview, save, cancel-revert).
- A `Select` control in the Settings Editor's Appearance section lets the user switch between
  `Generic` and `Catppuccin`.
- Tests cover the install/restore registry mutation and pass through the full frontend suite.
- Attribution to the upstream MIT-licensed source is documented.

## Implementation Notes
- Upstream source: `https://github.com/catppuccin/vscode-icons`, MIT licensed (Copyright (c) 2023
  Catppuccin, Copyright (c) 2023 thang-nm). Not published as an npm package, so a curated subset
  of `icons/mocha/*.svg` markup is reproduced verbatim in
  `frontend/src/themes/catppuccin-icons.ts` rather than imported as a dependency.
- Catppuccin's icons are stroke-based, multi-path/multi-group, fixed-palette-color, at
  `viewBox="0 0 16 16"` — structurally different from the single-path `currentColor` helpers in
  `frontend/src/components/icons.ts`, so they're rendered via `m.trust()` on the static, hardcoded
  inner markup (safe: content is a build-time constant, never user input) inside a
  Mithril-managed `svg` wrapper element (attributes handled normally, not string-templated).

## Agent Notes
- 2026-08-03 Claude Sonnet 5 (Copilot): Implemented end-to-end.
  - **Backend**: `IconTheme` enum + `Settings.icon_theme` field in `crates/fm-settings/src/lib.rs`
    (default `Generic`); `IconThemeDto` + `SettingsDto.icon_theme` in
    `crates/fm-transport-dto/src/settings.rs`; bidirectional mapping in
    `crates/fm-application/src/service.rs`'s `settings_to_dto`/`settings_from_dto`. No migration
    changes needed — `#[serde(default)]` on `Settings` makes old on-disk files load with
    `IconTheme::Generic` for the missing field. Verified via `cargo test -p fm-settings -p
    fm-application` (all green) and `cargo test --workspace`.
  - **OpenAPI/Orval**: regenerated via `pnpm run api:export` + `pnpm run api:generate`; new
    `frontend/src/api/generated/models/iconThemeDto.ts`, updated `settingsDto.ts`.
  - **Frontend model/mock/fixtures**: `iconTheme` added to `frontend/src/models/settings.ts`,
    `mock-file-manager-client.ts`'s default settings, and the two hand-built
    `fixtureSettings()` test helpers in `settings-editor.test.ts`/`settings-model.test.ts`.
    `http-file-manager-client.ts` needed no change (its DTO↔domain mapping is spread-based).
  - **Built**: `frontend/src/themes/catppuccin-icons.ts` — 27 vendored icon renderers (folder,
    file, symlink, ts/tsx/js/jsx, json, markdown, html, css, yaml, toml, rust, python, xml, csv,
    git-dotfiles, lock, log, font, text, plus image/audio/video/pdf/zip MIME fallbacks) and
    `installCatppuccinIconTheme()` / `restoreDefaultIconTheme()`, both defaulting to mutating the
    shared `entryIconRegistry` singleton (same pattern documented for 0085).
  - **Wired**: `app-shell.ts`'s `applyAppearance()` now installs/restores the icon theme
    alongside theme/font/format settings. `settings-editor.ts`'s Appearance section gained a
    `Select` (`Generic`/`Catppuccin`) next to date/size format.
  - **Documented**: new "Catppuccin icon theme" subsection in `docs/architecture/theming.md` with
    attribution and the design rationale for `m.trust()`.
  - **Tested**: new `frontend/src/themes/catppuccin-icons.test.ts` (5 tests: install overwrites
    kind/extension/mime icons, defaults to the shared singleton, restore reverts exactly to
    `createDefaultEntryIconRegistry()`'s values including removing Catppuccin-only extensions,
    and a render-shape assertion on the produced `svg` vnode). Full frontend suite:
    `pnpm exec vitest run` → 410/410 passing. `pnpm exec tsc --noEmit` clean.
    `pnpm run lint` (cargo fmt --check + clippy -D warnings + biome check) clean.
