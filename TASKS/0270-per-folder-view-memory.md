# 0270 Per-folder view memory

Status: open
Priority: low
Subsystem: backend, frontend
Depends on: 0029, 0134

## Context
Photos want grid view, Downloads wants date sort; users re-apply this every visit. Remember view mode, sort and columns per folder (or per folder pattern).

## Acceptance Criteria
- View mode, sort and visible columns are remembered per location and restored on navigation; an indicator and a 'forget for this folder' action exist.
- Stored in settings via the migration machinery (0030); bounded size with LRU pruning; works for remote locations keyed by connection + path.
- Opt-out setting; defaults unchanged for folders never customized.

## Implementation Notes
- Respect browser/Tauri parity: state is backend-owned settings, not localStorage.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. No work started.
