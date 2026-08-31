# 0075 Directory comparison and synchronization

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0047

## Context
`file-manager-coding-agent-spec.md` §16 milestone 5 and §37 (directory comparison and basic
synchronization are part of polished version 1). Total Commander parity feature.

## Acceptance Criteria
- Compare the two panes' directories, producing a per-entry status: only-left, only-right, newer,
  older, different size, identical, and type-mismatch.
- Comparison criteria are selectable: name only, size + timestamp, and content hash (the hash mode
  reuses 0077).
- Comparison of large trees is a cancellable job with progress, run in the engine — not in the UI.
- The result is presented in the panes with clear, non-colour-only status indicators (§29) and can
  be filtered to differences only.
- Synchronization proposes a concrete plan (copy left→right, right→left, delete, skip) that the user
  reviews and edits before anything runs; nothing is applied without confirmation (§35).
- Applying a plan runs through the operation engine with the normal conflict, progress and
  cancellation semantics.
- Integration tests: comparison correctness on a fixture pair, sync plan generation, cancellation,
  and a dry-run assertion that no files change until applied.

## Implementation Notes
- Recursive comparison must reuse the cycle-protected traversal from 0018/0040.
- Keep the comparison result a value object so a future "compare against a remote provider" needs no
  redesign (§6).

## Agent Notes
- 2026-08-13 Claude: Implemented end-to-end. Backend: new `fm-comparison` crate
  (`ComparisonCriteria` nameOnly/sizeAndTimestamp/contentHash, the 7-state `ComparisonStatus`,
  a `classify()` precedence function, a `ComparisonEngine` that streams batches from an
  iterative, provider-neutral two-root merge traversal — never recurses into a symlink or a
  directory pair that doesn't exist/match on both sides, which is what makes a symlink loop
  terminate without a separate device/inode cycle detector — a `ComparisonResultsStore` with
  differences-only paging, and pure `generate_sync_plan()` covering mirror-left-to-right,
  mirror-right-to-left and two-way-update modes). Content-hash mode streams SHA-256 per file
  (task 0077 should unify this with its own shared checksum implementation once it lands, per
  the implementation note above). `fm-events` gained `ComparisonResultsBatch` and an
  `OperationKindPayload::Compare` shadow-operation kind so a running comparison shows in the
  operation centre and shares the generic `/operations/{id}/cancel` route, mirroring how task
  0068's search already does this. `fm-application` wires the engine in, and `apply_sync_plan`
  turns a (possibly user-edited) plan into ordinary `copy`/permanently-confirmed `delete`
  operations through the existing engine — trash was rejected in favor of permanent delete
  because trash needs a platform capability browser/server mode does not have, and the sync-plan
  review step is itself the explicit confirmation `permanent_delete_confirmed` exists for.
  REST (`/api/v1/comparisons*`) and Tauri commands are thin wrappers, and the OpenAPI/Orval
  client was regenerated. Frontend: `FileManagerClient` gained the 5 comparison methods across
  the HTTP/Tauri/mock adapters (the mock walks the fixture directory tree bilaterally for a
  believable demo comparison); a `features/comparison/` module holds pure state
  (`comparison-state.ts`), a controller, a `DirectoryColumnDescriptor`-based status-badge column
  (non-colour-only per §29 — every status has its own text label and a full `title`/`aria-label`
  description) reused by both panes, and a `SyncPlanDialog` for review/per-row-edit before
  applying (spec §35). Wired into `app-shell.ts`: a toolbar "Compare panes" button, a
  differences-only checkbox (filters each compared pane's rows before the synthetic `..` row is
  added, reusing the existing quick-filter's scrollbar-sizing trade-off), a "Sync…" trigger, and
  the review dialog.
- 2026-08-13 Claude: Verified: `cargo test -p fm-comparison` (41 tests: 32 unit + 9 integration
  against real temp-directory fixtures, including a symlink-cycle and a cancellation-mid-traversal
  test), `cargo test -p fm-transport-dto` (94, incl. 5 new comparison DTO tests),
  `cargo test -p fm-application --test comparison_and_sync` (6, incl. a dry-run assertion that
  neither fixture changes until `apply_sync_plan` is called, and that cancelling through the
  generic `/operations/{id}/cancel`-equivalent path stops a running comparison),
  `cargo test -p fm-server --test comparison_routes` (4), and `cargo test --workspace` (926 passing
  across the whole workspace, after adding `fm-comparison` to `fm-test-support`'s `CRATE_LAYERS`
  fitness check — layer 2, alongside `fm-search`/`fm-archive` — which the existing
  `workspace_crates_respect_the_documented_layering` test caught immediately). `cargo clippy
  --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` are both clean. On the
  frontend: `vitest run` (980 passing, up from the pre-existing 916, across 64 new tests covering
  comparison-state, the controller, the status-badge column, the sync-plan dialog, the 3 client
  adapters, and the `comparison.resultsBatch` event-handler case), `tsc --noEmit`, `biome check .`,
  and a production `vite build` all clean. Manually exercised the mock runtime in a browser end to
  end: compare panes → per-row "only here" badges in both panes → differences-only toggle →
  "Review sync plan" dialog with the correct proposed actions and count → apply → dialog closes
  with no console errors → close comparison returns both panes to their plain view.
- 2026-08-13 Claude: Known gaps: the "compare" trigger always compares the first two panes in
  `paneOrder` at `sizeAndTimestamp` criteria (no UI yet to pick `nameOnly`/`contentHash` or an
  arbitrary pane pair, though the REST/Tauri surface and controller already accept any criteria);
  there is no dedicated command-palette/keybinding entry, only the toolbar button; and duplicate
  detection's staged size/partial-hash/full-hash strategy (0077) has not landed, so content-hash
  mode's per-file SHA-256 is a placeholder implementation flagged for unification once 0077 lands,
  as its own implementation note requires.
- 2026-08-16 Claude: Unified the content-hash implementation with task 0077, closing the placeholder
  flagged in the note above. `fm_comparison::engine::hash_entry` no longer carries its own SHA-256
  loop, `HASH_CHUNK_BYTES` constant or `sha2`/`AsyncReadExt` imports; it now delegates to
  `fm_checksum::hash_entry`, so the content-hash comparison mode and the checksum/duplicate features
  share one chunked, cancellable streaming hasher and cannot disagree about a digest. `fm-comparison`
  gained a `fm-checksum` dependency and dropped `sha2`. Because `fm-test-support`'s `CRATE_LAYERS`
  forbids same-layer edges, `fm-comparison` moved from layer 2 to a new layer 3 ("composite engines
  built from the layer-2 primitives"), with `fm-application`/`fm-test-support` shifting 3→4 and the
  hosts 4→5; `fm-checksum` stays at layer 2 beside `fm-search`/`fm-archive`. All 42 `fm-comparison`
  tests still pass unchanged, including
  `content_hash_criteria_distinguishes_identical_content_from_a_differing_timestamp`, which is the
  behavioural proof the swap is transparent.
