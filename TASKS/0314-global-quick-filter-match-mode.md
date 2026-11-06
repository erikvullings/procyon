# 0314 Global quick-filter match-mode toggle (fuzzy vs contains)

Status: done
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
- Done (commit 0c11dd11). Added `substringMatches()` to fuzzy-match.ts and a `matchMode` param
  (default `fuzzy`) on `compileQuickFilter`/`filterEntries`/`matchesQuickFilter` and the recursive
  walk's options, so globs/`;`/fuzzy all keep behaving as before in `fuzzy` mode.
- `reduceTypeahead` + `typeahead-controller.moveWithinMatches` switch between best-fuzzy and
  first-substring by mode, keeping the cursor and the filter in agreement (the 0310 principle).
- Mode is a single **global** `shellState.quickFilterMatchMode` (not per-tab), flipped by a button
  in `QuickFilterInput`; every open pane re-filters on the next redraw. `pane.ts` can't read `attrs`
  at the body-scoped controller-creation site, so it captures the latest mode in a `let` updated in
  the view.
- **Durable persistence deferred**: the concurrent transfer-recovery backend work was touching the
  backend settings DTO / OpenAPI, so v1 is in-memory-global only (follow-up noted in the task).
- **Entanglement note**: while implementing, the other agent's transfer-recovery commits
  (0299–0302) swept my uncommitted i18n files (en/es/fr/it/nl/pl/pt) into their commits; my source
  changes and `de.ts` were committed here. The `backend-codes.test.ts` failure at commit time is
  theirs (uncommitted `codes.rs` transfer codes without translations), not this task.
- Tests: full frontend suite green (3417 pass); tsc + biome clean.
