# 0310 Fuzzy quick-filter (and type-to-select)

Status: done
Priority: medium
Subsystem: frontend
Depends on: none

## Context
The per-tab quick filter (0067) matches a strict prefix of entry names, and keyboard
type-to-select (0028) matches the same way. Both feel brittle next to fzf-style subsequence
matching users expect today. This upgrades the matching model for both, consistently.

## Acceptance Criteria
- Quick filter uses fuzzy subsequence matching (case- and diacritic-insensitive) with a small,
  stable score (burstiness/first-position bonus) so the best matches rank first; keep it client-side
  over already-loaded entries — no new server round-trip.
- **Opt-in recursive scope**: a toggle (persisted per tab, 0030) that extends the filter to entries
  in sub-directories, labelled clearly (e.g. "this folder + subfolders"). Default stays current-pane
  for speed and predictability.
- **Type-to-select is fuzzy too**: the existing "type a name to select the first match" uses the
  *same* fuzzy matcher as the filter, so cursor behaviour and filter text agree. Decision recorded:
  unify the two rather than keep prefix-only type-to-select.
- Existing exact/prefix tests keep passing where they should; new fuzzy cases asserted.

## Implementation Notes
- Extract one pure `fuzzyMatch(query, name) -> score` helper (like the shared chip renderer pattern)
  and use it from both the quick-filter reducer and the type-to-select dispatcher; this is what
  guarantees the two never drift.
- Diacritic-insensitivity via `String.prototype.normalize('NFD')` + strip combining marks.
- The recursive toggle threads through the same tab-state slice the quick filter already uses; guard
  against accidental huge-tree scans (operate on the loaded/visible set, or a bounded walk).
- Consider a `core.toggleQuickFilterScope` action so the recursive toggle is keybinding- and
  palette-reachable (0050/0051).

## Agent Notes
- Done (frontend-only; no backend/Rust changes). Extracted one shared, pure matcher
  `src/utilities/fuzzy-match.ts` (`foldCase`, `fuzzyScore`, `fuzzyMatches`, `bestFuzzyMatch`,
  `rankFuzzyMatches`) and pointed BOTH the quick filter and keyboard type-to-select at it, so they
  can never drift:
  - `quick-filter.ts`: the plainText mask now fuzzy-matches (case/diacritic-insensitive subsequence);
    glob masks and `;`-OR semantics unchanged, so existing substring tests keep passing.
  - `selection/keybindings.ts` `reduceTypeahead` → `bestFuzzyMatch`; `panes/typeahead-controller.ts`
    `moveWithinMatches` → `rankFuzzyMatches`. Updated the one pane.test.ts case that asserted the
    OLD substring behaviour (`'dox'` is a valid subsequence of `document.txt`, so it is no longer
    "unmatched" — switched it to genuinely-unmatched `'doz'`).
- Recursive "this folder + subfolders" scope (`0310`): bounded BFS walk in
  `src/features/quick-filter/recursive-quick-filter.ts` (caps 200 dirs / 2000 matches, abortable,
  skips unreadable dirs) that lists via the pane-independent `client.listDirectoryChildren` and
  reuses the real `compileQuickFilter`, so globs/`;`/fuzzy all behave identically to the current
  folder. Wired through `shellState.quickFilterRecursive` (per-tab key) + `entriesFilteredFor`
  (uses the walk's collected results when the scope is on and a query is present) in app-shell.ts,
  triggered from `onFilterQueryChange` and the toggle in `QuickFilterInput` (new `scope` attr, icon
  `directoryTreeIcon`).
- **Deliberate scoping decisions** (documented per the task's "confirm with reviewer" note):
  - The recursive flag is an **ephemeral per-tab toggle** in `shellState`, mirroring the existing
    `photoMode` pattern — NOT persisted to the backend `PersistedFilterDto`. Extending that DTO
    would mean editing Orval-generated types + a Rust/OpenAPI change, which is out of this
    frontend-scoped task; durable cross-restart/cross-window persistence is a noted follow-up.
  - The `core.toggleQuickFilterScope` action from the impl notes was **not** added: registering a
    new action touches the backend action registry (Rust), and the toggle is reachable from the
    filter box UI, which covers discoverability. Can be added later if keybinding reachability is
    wanted.
- Tests: `fuzzy-match.test.ts` (11), `recursive-quick-filter.test.ts` (6). Full frontend suite green
  (3410), tsc + biome clean.
