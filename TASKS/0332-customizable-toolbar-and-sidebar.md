# 0332 Customizable toolbar and sidebar with arbitrary commands

Status: open
Priority: medium
Subsystem: frontend, settings
Depends on: 0049, 0094, 0319

## Context
Files lets the user customize the sidebar and toolbar, adding any command (e.g. "Copy path", "Create zip archive") and choosing which buttons show labels. Procyon has no equivalent: the toolbar is fixed in `app-shell.ts`, the compact overflow menu (0319) is hard-coded, and the sidebar (0139) is not user-configurable. Searched code and tasks: no matches.

The action registry (0049) already backs menus, the palette and shortcuts, so every command is addressable by ID. This task makes the toolbar and sidebar data-driven over that registry.

## Acceptance Criteria
- Settings lets the user add, remove and reorder toolbar buttons from the action registry, and set per-button label visibility (icon only, icon + text).
- Sidebar entries (favourites, tags, custom command items) can be added, removed and reordered from settings or a context menu.
- Any registered action can be placed; actions with unmet context requirements render disabled, not hidden.
- Config persists in settings (versioned, migrated) and round-trips through the existing settings migration machinery.
- The compact overflow (0319) reflects the customized toolbar: items that do not fit go to the overflow menu.
- Reset-to-default restores the current layout exactly.
- Browser and Tauri hosts behave the same (preserve parity).

## Implementation Notes
- Layout is a serializable tree/list over action IDs, owned by settings. Do not persist rendered DOM state.
- Frontend renders from the layout; keep the existing hard-coded buttons as the default layout so behaviour is unchanged until the user edits it.
- Plugin-contributed actions (0053) should be placeable too, once registered.

## Agent Notes
- Created from user request; nothing investigated yet. Decide default layout and settings schema before coding.
