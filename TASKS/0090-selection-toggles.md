# 0090 Total Commander-style selection toggles (invert, select/deselect by mask)

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: frontend
Depends on: 0028

## Context
Total Commander reserves the numeric keypad for selection: `Numpad *` inverts the selection,
`Numpad +` selects a group of files matching a mask (default `*.*`, i.e. everything), and
`Numpad -` deselects a group matching a mask.

Inspection of the current codebase found `core.invertSelection` is a genuine dead end, not just
missing a shortcut:
- `crates/fm-application/src/action.rs` registers `core.invertSelection` with `Vec::new()` — no
  default shortcut at all (contrast `core.selectAll`, which gets `Ctrl/Cmd+A`).
- The selection reducer's `'invert'` action (`frontend/src/features/selection/selection.ts`) is
  fully implemented and unit-tested (task 0028).
- But `frontend/src/features/panes/pane.ts`'s keydown handler, which maps `actionId ===
  'core.selectAll'` to `{ type: 'selectAll' }`, has **no matching case for
  `core.invertSelection`** — so even if a user rebound it in settings, nothing would happen. The
  reducer action is unreachable from the UI today.
- There is no select-by-mask / deselect-by-mask action or reducer case at all (`core.selectAll`
  selects everything unconditionally; there is nothing between "select all" and "select one").

## Acceptance Criteria
- `core.invertSelection` gets a default shortcut of `Numpad *` (fall back gracefully — see
  Implementation Notes — on keyboards/layouts without a numeric keypad).
- Wire `core.invertSelection` in `pane.ts`'s keydown handler to dispatch `{ type: 'invert' }` to the
  selection reducer (the missing piece — the reducer and action id already exist).
- New `core.selectByMask` / `core.deselectByMask` actions (default shortcuts `Numpad +` / `Numpad
  -`) prompt for a glob mask (default `*.*`, i.e. everything, matching Total Commander) and
  add/remove all *currently visible* entries matching it to/from the selection, without disturbing
  the rest of the selection (unlike `selectAll`/`invert`, this is additive/subtractive, not a full
  replacement) — new reducer action(s) in `selection.ts` (e.g. `{ type: 'selectByMask'; pattern:
  string }` / `{ type: 'deselectByMask'; pattern: string }`), reusing whatever glob-matching helper
  `fm-search`'s frontend-facing pieces or the existing quick-filter (task 0067) already have rather
  than writing a third glob matcher.
- All three respect the existing filtered/visible entry ordering (same `orderedEntryIds` contract as
  every other selection reducer action — see the repo-memory note on this).
- Tests: reducer unit tests for `selectByMask`/`deselectByMask` (including "no matches" and "all
  match" edge cases), a `pane.ts` keydown test asserting `Numpad *` dispatches `invert`, and backend
  action-registry tests for the new/changed default shortcuts.

## Implementation Notes
- Keyboard-event key names for the numeric keypad are locale/OS-dependent in some browsers; verify
  what `event.key` actually reports for numpad `*`/`+`/`-` across the target platforms (typically
  `"*"`, `"+"`, `"-"` regardless of NumLock, since `KeyboardEvent.key` reports the character, not
  the physical key) before hard-coding a `KeyChord`. Since these characters collide with the plain
  top-row `*`/`+`/`-` keys (no distinct "numpad" flag in the `key` string on most browsers/layouts),
  document whether that collision is acceptable (Total Commander itself only reacts to the numeric
  keypad specifically, using scan codes) or an intentional relaxation for this codebase.
- Check `frontend/src/features/quick-filter/` (task 0067) for existing pattern/glob-matching logic
  before adding a new one for mask-based select/deselect.

## Agent Notes

- 2026-08-11 codex: Registered and wired invert/select-by-mask/deselect-by-mask actions. Numpad
  `*`/`+`/`-` use character-key bindings, with `Shift+8` and `Shift+=` fallbacks for keypad-free
  keyboards; the intentional browser key collision is documented in README. Mask prompts default
  to Total Commander's `*.*`, which matches extensionless names too, and operate only on visible,
  non-parent entries while preserving hidden and unrelated selections.
- 2026-08-11 codex: Added 8 focused behavior tests (3 reducer, 2 glob matcher, 2 pane, 1 registry)
  and verified the three affected frontend files (95 tests) plus all 15 registry tests. The full
  `fm-application` suite progressed through its unit and integration tests but was stopped after the
  pre-existing `cancelling_cross_volume_fallback_leaves_the_source_tree` test hung for more than two
  minutes. The full frontend suite reached 844 passing tests
  but retains 4 unrelated failures in pre-existing theme, diagnostics CSS, HTTP operation mapping,
  and stale mock-action expectations; typecheck likewise retains 3 unrelated pre-existing errors.
