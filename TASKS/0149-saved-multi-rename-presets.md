# 0149 Saved Multi-Rename presets

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0072

## Context

Identified from a competitive feature scan against ForkLift (2026-08-19 product-page discussion).
ForkLift lets a user save a Multi-Rename configuration and reapply it later; fm's Multi-Rename
dialog (0072, `frontend/src/features/operations/multi-rename-rules.ts` +
`multi-rename-dialog.ts`) already has the full rule engine (search/replace, prefix/suffix,
sequence, case) but every session starts from scratch.

Deliberately scoped small: this is a quick win layered on an existing, already-tested feature — a
named-preset save/load/delete flow around the existing rule set, not a rule-engine change. No new
subsystem, no new backend concept beyond persisting a small piece of settings data.

## Acceptance Criteria

- The Multi-Rename dialog gains "Save as preset…" (name the current rule configuration) and a
  preset picker (load a saved configuration, replacing the current rule state) and "Delete preset".
- Presets persist across sessions via the existing settings service (`crates/fm-application/src/
  settings_mapping.rs`, `frontend/src/models/settings.ts`), not a separate storage mechanism —
  extend `Settings` with a `multiRenamePresets: MultiRenamePreset[]` (or equivalent) field, matching
  the settings service's existing versioned-JSON/forward-migration pattern.
- Loading a preset onto a new selection recomputes the live preview immediately (reuses the
  existing preview table — no separate preview logic for the preset path).
- Preset names are unique; saving under an existing name prompts to overwrite rather than silently
  duplicating or silently failing.
- Deleting a preset requires no confirmation beyond the delete action itself being deliberate (it's
  reversible in spirit — recreating a preset is cheap — so this doesn't need Trash-style undo).
- Tests: preset save/load/delete round-trip (Vitest), settings-migration test if the `Settings`
  shape changes (matching the pattern other settings-field additions already use), and a test that
  loading a preset reproduces the exact same live-preview output as manually re-entering the same
  rules.

## Implementation Notes

- Rule state is already a pure `(entries, rules) → proposed names` function per 0072's own design
  note — a preset is just a saved `rules` value, so this should not need to touch
  `multi-rename-rules.ts`'s core engine at all, only `multi-rename-dialog.ts`'s UI state and the
  settings model.
- Follow whatever forward-migration convention the settings service already uses for adding a new
  field (see recent settings-shape changes in `crates/fm-application/src/settings_mapping.rs`) so
  older on-disk settings files load cleanly without the new field.

## Agent Notes

- Initial task setup. No execution attempts recorded yet.
- 2026-08-27 Copilot: Added versioned settings persistence and transport DTOs for named
  multi-rename presets, including a v4-to-v5 migration that defaults older files to an empty preset
  list. Added save, explicitly confirmed overwrite, immediate load/preview, and deliberate delete
  controls to the existing dialog; preset mutations are single-flight to prevent stale
  whole-settings updates from losing data. Added 6 Vitest dialog tests (14 total in the file) and 1
  Rust migration test; the settings persistence round-trip now includes a complete preset.
  Verified the 116-file/1474-test frontend suite, all 1323 Rust tests plus doctests, frontend
  typecheck, API regeneration, and full lint. The unrelated script suite remains 38/40 because its
  CI-command and desktop-identifier assertions are stale against the repository's current
  configuration.
