# 0319 Compact window toolbar

Status: done
Priority: medium
Subsystem: frontend
Depends on: none

## Context
The window toolbar showed ~10 equal-weight buttons above tabs, path row and column header: heavy
chrome. User feedback: keep back/forward/up/search; overflow menu after search; workspace switcher
and pinned command palette on the right; make it a setting. Pane-row icons are tab-specific and
stay; only the synchronized-browsing toggle needed clear on/off states.

## Acceptance Criteria
- `compactToolbar` setting (Settings → Appearance, default on) persisted by the backend.
- Compact: back, forward, up, search, "⋯" overflow, … workspace switcher, ⌘, activity indicator.
- Overflow menu holds content search, themes, add to basket, basket, compare, settings, help.
- Full layout (setting off) is unchanged.
- Sync-browsing toggle: link icon + accent when on (also "apart"), struck-through link + muted when off.

## Implementation Notes
- Rust: `fm-settings::Settings.compact_toolbar` → `SettingsDto` → mapping; OpenAPI/Orval regenerated.
- `features/toolbar/toolbar-overflow.ts`; `app-shell.ts` builds `overflowItems` and gates the inline buttons.
- A missing field (older payloads) means the full toolbar (`=== true` check).
- app-shell tests spy `MockFileManagerClient.getSettings` to keep the full toolbar; one new test covers compact.

## Agent Notes
- 2026-11-09 claude: implemented as above; frontend 3466 tests, tsc and Biome clean; Rust settings/dto tests pass.
  Not verified in a real Tauri window (macOS overlay drag region with the overflow menu, narrow widths).
