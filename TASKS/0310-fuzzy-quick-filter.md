# 0310 Fuzzy quick-filter (and type-to-select)

Status: open
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
- (none yet)
