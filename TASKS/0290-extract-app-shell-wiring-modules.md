# 0290 Extract feature wiring from app-shell.ts

Status: open
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
