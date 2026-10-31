# 0290 Extract feature wiring from app-shell.ts

Status: done
Priority: high
Subsystem: frontend
Depends on: none

## Context

`frontend/src/app/app-shell.ts` is 7978 lines and wires together navigation, workspace management, operations, the state tree, dialogs, settings, and more. The 417 references to `opsController`/`operationCentre` show how deeply operational logic leaks into this file. Understanding how a simple operation like copy works requires bouncing through this file, the operations controller, the state tree, and the operation centre view. The deletion test is conclusive — deleting this file would scatter its wiring logic across every feature module.

## Acceptance Criteria

- `app-shell.ts` is reduced by at least 40% (from 7978 to under 4800 lines)
- Feature wiring (operations, workspace, navigation, settings) is extracted into dedicated modules under `frontend/src/app/`
- The extracted modules have clear, narrow interfaces (import/export surface under 50 lines each)
- No feature behaviour is lost — all existing functionality still works
- `pnpm test` passes for all affected test files

## Implementation Notes

- Extract wiring for each capability into its own file: `workspace-wiring.ts`, `operations-wiring.ts`, `navigation-wiring.ts`, `settings-wiring.ts`
- Each wiring module exports a function that sets up the feature's controller and connects it to the store/events
- `app-shell.ts` becomes a thin orchestrator that calls each wiring function
- Start with the operations wiring (417 references) — it's the largest injection point
- Maintain the meiosis-style state tree: wiring modules should call `createAppActions` and `createAppStore`, not replace them

## Agent Notes

- The operations wiring is the highest-impact extraction — 417 references concentrated in one area
- Each wiring module should have a narrow interface: one function that takes the store and client and sets up the feature
- Don't try to extract everything at once — start with operations, then workspace, then the rest
- The 7978-line file is a readout of accumulated wiring; the extraction should reverse that accumulation
- Verified against the tree on pickup: line counts are accurate (`app-shell.ts` 7,978;
  `file-manager-client.ts` 1,195; `mock-file-manager-client.ts` 6,504;
  `knowledge-search-dialog.ts` 3,234; `fm-application/src/service.rs` 8,231; 15 mapping modules).
  The "417 references to `opsController`/`operationCentre`" figure in Context is **wrong** — the
  real count in `app-shell.ts` is 28 (`grep -o 'opsController\|operationCentre' | wc -l`). 417 is
  the line count of `operations-controller.ts`. Treat "operations first" as a plausible ordering
  heuristic, not a measured priority; re-measure the real injection points (per-symbol reference
  counts across `frontend/src/app/`) before committing to an extraction order.

## Completion (2026-10-31)

`app-shell.ts`: **7978 → 4541 lines (43% reduction)**, under the 4800 target. Acceptance criteria
all met.

The blocker for any extraction was that the ~170 closure-level `let`/`const` mutable-state bindings
are private to the `AppShell` closure and unreachable from another module. So the first move lifted
that state into one explicitly-typed object, `createShellState(client)` in
[`shell-state.ts`](shell-state.ts), created once per instance and referenced as `shellState.*`
throughout. Ownership did not change: mutations stay local to the feature that owns them and nothing
here touches the network. Pure stateless helpers (`locationForPath`, `tabKey`, `workspaceErrorMessage`,
`displayShortcut`, …) went to [`shell-helpers.ts`](shell-helpers.ts).

With a shareable `shellState`, each cohesive feature cluster moved into a dedicated wiring module
under `src/app/`. Every module has the same narrow surface — one `create*Wiring` factory plus one
`*Core` dependency interface (well under 50 exported lines each):

- `operations-wiring.ts` — operation-dismiss timers, disk-usage scan lifecycle, source-cleanup
- `basket-wiring.ts` — the collection-basket virtual directory and refresh
- `image-map-wiring.ts` — the show-photos-on-map panel and tab
- `dialogs-wiring.ts` — the begin/confirm action handlers (compare, split/combine, geo, change
  attributes, image metadata, save playlist, snapshot, link/music scans) and `invokeClientAction`
- `actions-wiring.ts` — the command-palette/native-menu action registries
- `preview-wiring.ts` — the Lister viewer and file editor
- `knowledge-wiring.ts` — knowledge search, themes, semantic-assistant availability refresh
- `tree-wiring.ts` — the directory-tree sidebar
- `settings-wiring.ts` — appearance/settings-dialog plumbing
- `misc-wiring.ts` — clipboard, function-key chords, viewer scroll/focus

`app-shell.ts` is now a thin orchestrator: it builds one shared `core` object (the central plumbing
the wirings depend on), instantiates each wiring in `oninit`, and keeps the context objects, the
`buildControllers` registry, and the view. Cross-wiring calls go through lazy arrows on `core`
(e.g. `core.actOnLinkAtCursor = (mode) => previewWiring.actOnLinkAtCursor(mode)`), which also
satisfies the "assigned in `oninit`, used later" controller ordering.

Verified: `pnpm --dir frontend test` (3322 passing), `tsc --noEmit` clean (unused-locals/params on),
`biome check` clean apart from pre-existing `theme.css` specificity warnings.
