# 0308 Disk-cleanup report

Status: open
Priority: medium
Subsystem: frontend, backend
Depends on: none

## Context
The disk-usage work (0230–0234) gives a treemap and aggregate totals, but nothing that answers the
practical questions: "what file types are eating my space?", "biggest files?", "old/unused files?",
"empty folders?". Tools like CleanMyMac/WinDirStat succeed on exactly these actionable lists.

## Acceptance Criteria
- A report view over an already-listed directory (bounded, paginated — never scan unbounded trees in
  the pane) with tabs: by extension/type, largest files, oldest (last-modified) files, empty dirs,
  and obvious junk (`.DS_Store`, `Thumbs.db`, `node_modules`, caches) — each with counts + totals.
- Select-and-act: trash/permanent-delete the selection through the existing operation engine and
  conflict/undo flow (0043/0044/0160); nothing is removed without the standard confirmation.
- Reuses the bounded aggregate-totals path (0097) and disk-usage scanner (0234) rather than a new
  listing round-trip where avoidable.
- Cross-host parity; respects remote/VFS providers without forcing full recursive enumeration.

## Implementation Notes
- Grouping/aggregation happens on already-loaded entries; "largest/oldest" are sorts over the current
  directory, not recursive scans (offer an explicit opt-in recursive scan as a separate, clearly
  labelled action if ever needed).
- Junk patterns are configurable, not hard-coded.

## Agent Notes
- (none yet)
