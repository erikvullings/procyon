# 0098 Frontend i18n with translate.js

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: none

## Context
The frontend currently embeds user-visible English strings throughout Mithril components and
feature models. Add a small internationalisation layer using `translate.js` so UI copy can be
collected into locale catalogues and switched at runtime without moving application logic into
components. English remains the initial and fallback locale; add one second locale to prove that
the architecture and runtime switching work end to end.

`translate.js` is intentionally minimal: it supports placeholders, numeric/fallback
pluralisation, one-level sub-keys, optional aliases, VDOM-safe array output and locale switching by
replacing `t.keys`. It does not support deep keys, locale-aware plural rules or gender variants.
Keep catalogue structure and copy requirements within those constraints; do not build a parallel
i18n framework around it.

## Acceptance Criteria
- `translate.js` is added to the frontend dependencies and wrapped by a small typed i18n module
  that owns supported locale identifiers, catalogue selection, the configured translator and the
  fallback/missing-key policy.
- English user-visible strings are moved into a central English catalogue with stable, shallow
  keys. At minimum, the application shell, pane/status UI, command/action labels, settings UI,
  operation/conflict UI, file viewer and common empty/error states use translations rather than
  inline copy.
- At least one complete second locale is included and can be selected at runtime without a page
  reload. Changing locale redraws the active Mithril UI and updates all translated surfaces.
- Locale preference is exposed through the existing settings model/editor and persists through
  the existing settings service. Browser (Axum) and Tauri adapters remain in parity; regenerate
  OpenAPI and Orval output if the settings DTO changes rather than editing generated files.
- Placeholders and count-dependent copy use the library API. Where Mithril vnodes are interpolated,
  use `t.arr()` (or an `array: true` translator) rather than converting vnodes to strings.
- Missing keys degrade predictably to the English value or key, are conspicuous in development,
  and do not crash production. Catalogue parity is checked by a test so additions cannot silently
  leave the second locale incomplete.
- Document the translation-key naming convention and the library limitations (single-level
  sub-keys and basic pluralisation) for future contributors.
- Tests cover initial locale selection, persisted preference, runtime switching, interpolation,
  pluralisation, missing-key behaviour, catalogue parity and representative component redraws.

## Implementation Notes
- Inspect `frontend/src/features/settings/`, `frontend/src/app/` and shared UI primitives before
  choosing module boundaries. Keep locale state/actions outside Mithril components and expose only
  the minimal translator and locale-switch action they need.
- Prefer flat semantic keys or one-level groups such as `t('button', 'save')`; deep dotted key
  lookup is not supported by `translate.js`.
- Decide explicitly whether aliases (`resolveAliases: true`) are useful. Avoid them unless they
  materially reduce duplicated copy, since eager alias resolution can make runtime catalogue
  replacement less obvious.
- Translate application-owned UI copy. Backend error identifiers should remain typed/stable and
  be mapped to translated presentation copy at the frontend boundary; do not localise protocol
  values or generated API models.
- Audit accessibility labels, titles and status announcements as well as visible text.

## Agent Notes
- 2026-08-05 codex: Created from the request to add i18n using `translate.js`. Scope deliberately
  includes a second complete locale and persisted runtime switching so this is demonstrably more
  than extracting English constants. No locale other than English was prescribed; choose one with
  the user when implementation starts if product direction requires a specific language.
- 2026-08-16 opencode: Implemented end-to-end i18n layer. Backend: `Language`/`LanguageDto` enum in
  fm-settings/fm-transport-dto, settings mapping, schema v4 migration with default locale.
  Frontend: `translate.js` added as dependency; `src/i18n/` module with typed `Translator`, English
  and Dutch catalogues covering shell, settings, pane, operation, viewer, and state surfaces. All
  hardcoded UI strings in app-shell.ts and settings-editor.ts replaced with `t()` calls. Language
  selector added to settings Appearance section; locale persists through settings service and
  applies at startup via `applyAppearance` → `setLocale`. Catalogue parity test prevents NL from
  falling behind EN. **Verified**: 18 i18n tests (initial locale, runtime switching, interpolation,
  pluralisation, missing-key, catalogue parity, 2 component redraws), full suite 1135 tests green,
  `tsc --noEmit` clean. Known: operation dialog UIs (conflict, create directory/file, permanent
  delete) and panes Chrome (favourites, tabs) are not yet wired — they use hardcoded strings
  because their catalogue keys exist but the components haven't been converted.
- 2026-08-17 codex: Completed the outstanding frontend work and corrected the catalogue typing.
  English now defines the exact translation schema; every other locale must satisfy the same
  groups, keys, and plural variants, and `t()` rejects unknown group/sub-key pairs during `tsc`.
  Locale changes redraw directly, while Vite catalogue updates rebuild the active translator so
  edits are visible without stale singleton state. Wired the remaining core operation/conflict
  dialogs and controls, pane/tab chrome, file-viewer controls and states, grid empty/error states,
  titlebar, and frontend-owned favourite actions. Documented shallow-key and translate.js limits.
  Added 3 regression tests for compile-time key rejection, core-action localisation, and a live
  Dutch component redraw.
  Verified focused i18n/component tests, all 110 frontend test files (1289 passed, 1 skipped),
  frontend typecheck, Biome frontend lint (one pre-existing CSS specificity warning), and
  `git diff --check`.
