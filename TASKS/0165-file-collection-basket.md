# 0165 File collection basket

Status: open
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
