# 0158 Safe large structured-file editing

Status: open
Priority: low
Owner: unassigned
Agent: unassigned
Subsystem: frontend, backend, operations
Depends on: 0100

## Context

After 0100 makes multi-gigabyte CSV and JSON files safely viewable, users may want to correct a
cell/property or insert/delete a row/object. These formats cannot generally be edited in place:
changing one value can shift every following byte, and a crash, disk-full condition, stale source,
or serialization mistake could destroy an irreplaceable multi-gigabyte file.

This task is intentionally gated. It does **not** assume that large-file editing should ship. Its
first outcome is an evidence-backed go/no-go decision. Until that decision is explicitly recorded,
all multi-gigabyte structured views remain read-only. If the safety invariants cannot be proven at
reasonable complexity, cancel this task and keep Procyon as a viewer plus "Open with..." launcher.

Even after a go decision, the first releasable scope writes a modified **new file** (`Save As` /
`Export modified copy`). It never replaces the source. Replacing the original would require a
separate future task and product approval informed by real-world use of the copy-only workflow.

## Acceptance Criteria

### Phase A: mandatory safety gate

- Write a short decision record covering corruption, crash consistency, disk exhaustion,
  cancellation, concurrent modification, symlinks, provider capability differences, remote
  disconnects, format-preservation limits, recovery UX, and worst-case rewrite time/free-space
  requirements.
- Define the supported edit shapes narrowly. Candidates are CSV/TSV records, NDJSON objects, and
  optionally elements of a top-level JSON array. Arbitrary deeply nested giant JSON and all Excel
  editing are explicitly out of scope.
- Build a throwaway backend prototype that stores edits as an immutable overlay keyed by source
  revision and stable record/value byte spans. It must never open the source for writing or use a
  writable memory map.
- Prototype export as a cancellable operation-engine job: stream unchanged source ranges and
  serialized edits into a new destination, flush/close it, reopen and validate it, and only then
  report success. Partial output is clearly identified and recoverable/removable; the source is
  untouched on every failure path.
- Demonstrate fault-injection tests for cancellation at multiple offsets, disk full/short write,
  process-style interruption before finalization, malformed edits, source revision change,
  destination conflict, and remote read/write failure.
- Demonstrate byte-preservation tests: all unedited source ranges are byte-identical in the output,
  edited records parse to the requested values, record order is preserved, and newline/delimiter/
  quote conventions are retained wherever representable.
- Measure peak memory and temporary/free-space requirements using generated large fixtures. Memory
  must stay bounded independently of source size, and the UI must calculate/explain required disk
  space before starting an export.
- Record an explicit product go/no-go decision in Agent Notes after reviewing the prototype and
  evidence. No production edit controls or write endpoints may be added before this gate. A no-go
  decision cancels this task with the reasons preserved rather than weakening the criteria.

### Phase B: only after an explicit go decision

- Add staged editing for the approved formats only: edit a value, insert/delete a record, dirty
  state, validation errors, and undo/redo. The source remains read-only while editing.
- Changes are kept as a bounded patch overlay, not by loading or reconstructing the complete file
  in frontend memory. Scrolling/index eviction must not discard pending changes.
- The only save action is clearly labelled `Export modified copy` or `Save As`; it requires a new
  destination and refuses the source location. There is no overwrite-source escape hatch, hidden
  advanced flag, or automatic replacement.
- Export refuses to start if the source revision no longer matches the viewing/indexing session.
  The user may reopen/rebase deliberately, but stale patches are never applied automatically.
- Export runs through the operation queue with progress, cancellation, failure details, and a
  durable audit/history entry. Failure leaves the original intact and never presents a partial
  destination as complete.
- The completed output is reparsed before success is shown. JSON output must be valid for its
  supported shape; CSV output must preserve the selected dialect and expected field structure.
- Closing a dirty editing session requires discard/cancel confirmation. Recovery of the patch
  overlay after an application crash is either implemented and tested or explicitly declined with
  a warning before editing begins.
- End-to-end tests cover local and at least one non-local provider, HTTP/Tauri parity, undo/redo,
  row/object insertion and deletion, UTF-8/multi-byte boundaries, huge individual records, and
  every Phase A failure class against the production path.

## Implementation Notes

- Reuse 0100's source revision, record index, and structured session. Do not create a second parser
  whose interpretation of row/value boundaries can drift from the viewer.
- The existing bounded text editor in `crates/fm-application/src/file_editor.rs` hashes and replaces
  whole files up to 3 MiB. Its optimistic-conflict and sibling-temporary ideas are useful, but its
  whole-file buffer and ordinary request/response save path are not suitable for multi-gigabyte
  rewrites.
- Long-running export is a mutating job under the operation engine, not a blocking REST/Tauri
  command. REST and Tauri commands remain thin adapters to the same application service.
- For CSV, serialize only changed/inserted records with the approved dialect; copy untouched byte
  ranges exactly. For JSON, preserve untouched byte ranges and replace only parser-proven value or
  element spans. Do not attempt global pretty-printing during export.
- Prefer conservatism over feature count. A useful read-only viewer plus external-editor action is
  an acceptable permanent product boundary.

## Agent Notes

- 2026-08-26: Created from direct product concern that a faulty editor could corrupt multi-GB JSON
  and lose user trust. The task is therefore a safety-gated copy-only workflow, not authorization
  to replace originals. Phase A may legitimately end in cancellation; production controls require
  an explicit recorded go decision.
