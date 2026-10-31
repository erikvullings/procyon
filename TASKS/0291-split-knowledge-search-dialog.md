# 0291 Split knowledge-search-dialog.ts monolith

Status: open
Priority: high
Subsystem: frontend
Depends on: none

## Context

`frontend/src/features/semantic/knowledge-search-dialog.ts` is 3234 lines and handles search, evidence display, answer generation, citation resolution, scope classification, and error handling. It exports dozens of functions but the implementation is a monolithic blob with no internal seams. Understanding any one aspect (e.g. citation rendering) requires reading through unrelated code.

## Acceptance Criteria

- `knowledge-search-dialog.ts` is reduced to under 500 lines
- 3-5 focused sub-modules are created under `frontend/src/features/semantic/`:
  - A search controller (~200 lines)
  - An evidence renderer (~200-300 lines)
  - An answer generator (~100-200 lines)
  - A citation resolver (~100 lines)
  - A scope classifier (~50-100 lines)
- All existing exported functions are preserved (no API breakage)
- `pnpm test` passes for the semantic feature tests

## Implementation Notes

- Each sub-module should have a single, focused responsibility
- The search controller coordinates the others but doesn't implement evidence rendering or answer generation logic
- Sub-modules should import each other through their interfaces, not through internal details
- Start with the evidence renderer (large, independent, clear boundaries) — it's the easiest split
- The `knowledge-search-dialog.ts` file becomes a thin coordinator that imports and wires sub-modules

## Agent Notes

- The evidence renderer (`evidence-provenance.ts`, `evidence-title.ts`, `media-evidence.ts`) already exist as separate files — they should be consolidated into the new evidence renderer module
- `rag-ask-dialog.ts` (839 lines) can stay as-is or be further split; it's a separate concern from the knowledge search
- The `document-summary-dialog.ts` (351 lines) is smaller and already focused — no split needed
- Watch for cross-references between the target file and other semantic features that may need updating
