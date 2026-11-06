# 0314 Global quick-filter match-mode toggle (fuzzy vs contains)

Status: open
Priority: medium
Subsystem: frontend
Depends on: 0310

## Context
0310 made the quick filter and keyboard type-to-select fuzzy (subsequence) by default. That's more
forgiving, but fuzzy is looser than the old substring match: `abc` now matches `a_x_b_x_c`, and
because the quick filter is a show/hide filter (not a ranked list) it *widens* the visible set
rather than just re-ordering it. Users who know the exact keyword can get "too many results."

This adds a single **global** toggle between `fuzzy` (0310 default) and `contains` (the old
substring behaviour), so both callers can be switched together — keeping the 0310 invariant that
the cursor and the filter agree.

## Acceptance Criteria
- One global match mode (app-wide, not per tab), defaulting to `fuzzy`, held in `shellState` and
  read by `compileQuickFilter`/`filterEntries` and the recursive walk, and by the type-to-select
  paths (`reduceTypeahead`, `moveWithinMatches`).
- `contains` mode uses substring matching (`foldCase(name).includes(foldCase(query))`); glob masks
  and `;`-separated OR masks are unchanged in both modes.
- A toggle button in the quick-filter box (`QuickFilterInput`) shows the active mode, flips it, and
  on redraw every open tab/pane reflects the new mode immediately.
- Type-to-select follows the same mode so cursor behaviour and filter text never disagree (0310
  principle preserved).

## Implementation Notes
- Keep the mode global in `shellState.quickFilterMatchMode` (one value for all tabs), mirroring how
  `theme`/`platform` are app-level rather than per-tab. The toggle just flips it and `m.redraw()`s.
- Thread `mode` as an **optional** param (default `fuzzy`) on `compileQuickFilter`/`filterEntries`/
  `matchesQuickFilter` so the many existing callers keep passing tests unchanged; only the
  filter/typeahead call sites that must honour the global setting pass it explicitly.
- The recursive walk takes the mode via its options (same default `fuzzy`) so a `contains` filter
  still widens across subfolders consistently.
- Persistence across restarts is intentionally deferred: a durable global pref would extend the
  backend settings DTO / OpenAPI (and collide with the concurrent transfer-recovery backend work),
  so v1 is in-memory-global. Note as a follow-up.
- Scope is the quick filter + type-to-select only; find-files / content-search keep their own query
  semantics.

## Agent Notes
- (none yet)
