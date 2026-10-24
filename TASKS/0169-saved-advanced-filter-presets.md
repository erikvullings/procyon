# 0169 Saved advanced filter presets

Status: done
Priority: low
Subsystem: frontend, settings
Depends on: 0030, 0067

## Context

Quick Filter provides an ad hoc text match, while 0129 tracks Total Commander's saved-filter feature
only as an untriaged candidate. Users should be able to save reusable pane filters without launching
a recursive search or smart folder.

## Acceptance Criteria

- Quick Filter supports a structured advanced mode for name/glob, type, size, modified age, hidden
  state, and platform-aware executable status where available.
- Users can save the current filter under a unique name, load it in either pane, rename it, and delete
  it.
- Applying a preset filters the current directory's loaded/paged results without recursively walking
  subdirectories.
- Active filter criteria remain visible and can be cleared in one action; unsupported criteria are
  explained rather than ignored.
- Presets persist through the existing versioned settings model and older settings migrate to an
  empty preset list.
- Tests cover predicate combinations, preset persistence, overwrite confirmation, provider metadata
  gaps, paging interaction, and keyboard accessibility.

## Implementation Notes

- Split from the Ctrl+F12 candidate in 0129; update that parent task when this feature is completed.
- Keep this distinct from 0162: filters constrain the current directory view, while smart folders run
  reusable searches across a scope.
- Reuse the saved-preset UX and migration patterns established by 0149.

## Agent Notes

- 2026-08-28: Promoted from 0129 into a standalone task during the product feature review.
- 2026-10-24 Claude: Implemented end to end.
  - Backend: `Settings.filter_presets: Vec<FilterPreset>` (`FilterCriteria`: name masks, kind,
    min/max size, modified within/older-than days, hidden and executable tri-states) in
    `fm-settings`, mirrored by `FilterPresetDto` & co. in `fm-transport-dto` and
    `settings_mapping.rs`. No schema bump: `#[serde(default)]` makes older files load with an empty
    list (tested for v1/v4/v8/current). `sanitize_filter_presets` runs on load and save: trims
    names, drops blank/case-insensitive duplicates, bounds count (100), name and mask length, and
    clamps numbers to the JS-safe integer range. OpenAPI and Orval client regenerated.
  - Frontend (`features/quick-filter/`): pure `advanced-filter.ts` (predicate, notices, size
    parse/format), `filter-presets.ts` (save/overwrite/rename/delete), `advanced-filter-store.ts`
    (per-tab criteria + panel state, rekeyed on tab move), `advanced-filter-bar.ts` (chips, form,
    preset controls; native labelled controls, Esc clears all). The quick-filter box gets a toggle
    button; `entriesFilteredFor` applies quick text then structured criteria (cache keyed on the
    criteria reference) so it only ever filters the loaded entries. Esc / chip "Clear filter" /
    box clear reset text, criteria and panel in one action. Presets live in settings via
    `updateLocationSettings` (the settings editor save preserves `filterPresets` like
    `multiRenamePresets`). Mock/HTTP/Tauri share the same `SettingsDto`, so parity is via the
    mapper; mock default is an empty list. i18n added for all 8 locales.
  - Unsupported criteria are explained, not ignored: `sizeUnavailable`, `modifiedUnavailable`,
    `noHiddenLoaded`, `executableByExtension`, `loadedEntriesOnly` (paging).
  - Verified: 14 new Vitest tests in `advanced-filter.test.ts`, 5 in `filter-presets.test.ts`, 16 (incl. store) in
    `advanced-filter-bar.test.ts`, 1 in `settings-mapping.test.ts`; Rust: 2 new `fm-settings` tests
    plus the extended full-settings round trip, 1 new `settings_mapping` test. Full frontend suite
    3149/3149, tsc clean, `pnpm run lint` clean, Rust workspace suite green.
  - Known gaps: advanced criteria are per-tab UI state and not restored after reload (only saved
    presets persist; the plain text query still persists as before); "executable" is an extension
    heuristic, not the Unix exec bit (listings carry none); no dedicated shortcut for Ctrl+F12 (use
    the filter box toggle); preset prompts use `window.prompt/confirm` like multi-rename presets.
