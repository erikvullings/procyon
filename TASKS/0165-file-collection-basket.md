# 0165 File collection basket

Status: done
Priority: medium
Subsystem: frontend, backend, operations
Depends on: 0035, 0048, 0108

## Context

Clipboard selection is replaced by the next copy/cut action and is tied to immediate transfer intent.
A persistent collection basket should let users gather entries from several folders, tabs, and
providers before applying one deliberate action to the collection.

## Acceptance Criteria

- Users can add/remove selections, inspect the basket, clear it, and restore it after navigation or
  restart according to an explicit persistence setting.
- Basket items retain stable provider/location references and clearly report missing, moved, or stale
  entries.
- Copy, move, checksum, archive, and delete actions can consume either the complete basket or a
  selected subset after showing the normal operation preview/confirmation.
- Adding the same stable entry twice does not create accidental duplicate work.
- The basket never stores credentials and does not hold remote connections open.
- Large collections are virtualized and all resulting mutations use the existing operation engine
  and cross-provider planner.
- Tests cover mixed providers, duplicates, stale items, persistence, partial selection, operation
  cancellation, and accessible keyboard operation.

## Implementation Notes

- Model the basket as references plus display snapshots, not copied entry contents.
- Keep it distinct from clipboard cut semantics: collecting an item must never mark or mutate its
  source.
- Consider workspace-scoped baskets so unrelated windows do not unexpectedly share selections.

## Agent Notes

- 2026-08-28: Created from the product feature review as a multi-location workflow that complements,
  rather than replaces, clipboard operations.
- 2026-09-30: Added a workspace-scoped collection tab opened from the shopping-basket toolbar tool.
  References are deduplicated by provider and stable ID, saved locally per workspace, and
  identity-checked on opening and before actions. Missing entries are marked "Missing or moved" unless the
  same stable identity is visible at its new location; changed identities are stale. Basket actions
  use the existing operation confirmation, archive, checksum, and delete flows, with explicitly
  checked entries only. Adding a parent folder replaces collected descendants; adding a child
  under an existing folder is blocked, and actions reject overlapping restored selections.
  Remote providers without object-stable IDs use repeatable path IDs plus available size and
  modification time; identical-metadata replacements cannot be distinguished. The list is
  virtualized and uses keyboard-accessible checkboxes. Basket folder totals are calculated
  asynchronously using the existing folder-size capability.
- Directory navigation, comparison, and directory-only function keys are unavailable while the
  basket tab is active; F5 still collects from the other pane. Basket icon controls share the
  workspace toolbar's button component, with selection-state transitions kept instantaneous to
  avoid flashing the action row.
