# 0307 Watched-folder auto-organizer

Status: open
Priority: high
Subsystem: backend, frontend
Depends on: none

## Context
Users love "drop files in and they file themselves away." Procyon has the pieces — VFS watching
(0020/0031), the cancellable/resumable operation engine (0035), rules/DSL parsers (0205), and a
path toward declarative recipes (0167, not built). What's missing is a *standing rule* that acts on
new arrivals automatically.

## Acceptance Criteria
- A watched-folder definition: a source directory + an ordered rule set (match by extension, size,
  age, content type, or semantic category) and an action (move/copy/trash to target, tag, set
  Spotlight comment, run a plugin).
- Setup scans the existing folder and uses the configured LLM to propose a shallow folder structure
  and per-file moves. Preview is read-only. Show the proposed destinations and unmatched files,
  let the user edit or exclude assignments, and apply only explicitly approved moves. Reuse the
  approved categories and rules for future arrivals.
- Standing rules classify completed new arrivals. Retain the original file in the watched folder
  for a configurable grace period (48 hours by default for Downloads), so Finder and native Open
  dialogs can still find it. Apply the same grace period to recent files included in initial cleanup.
  Show the pending destination and due time. Temporary aliases and symbolic links are out of scope.
- Pending filing survives restart. Pause prevents new submissions, and moves are idempotent so a
  re-arrival or restart does not double-act. Recheck file identity and destination at execution time.
- Target directories resolved through the existing location/VFS layer (local + remote providers).
- A management UI (add/pause/remove watched folders, reorder rules, dry-run preview) and clear
  failure handling (target full/unavailable → notify, don't lose the file).
- Cross-host parity; long-running evaluation is a job, never a blocking call (spec §3 layering).

## Implementation Notes
- Model watched folders as durable settings (0030) under a new `watchedFolders` key; the watcher
  subscribes to the existing event bus rather than adding a second watch mechanism.
- Reuse `OperationsCoordinator`/operation jobs for the mutations so everything stays cancellable,
  undoable (0160) and visible in the operation centre (0036).
- Design the rule DSL to be a strict subset of what 0167 recipes will expose, so recipes can later
  *include* a watched-folder trigger rather than duplicating it.
- Security: never follow symlinks blindly; targets must be within an allowed set the user configures.

## Agent Notes
- The application layer can run a cancellable, read-only initial preview through the configured
  chat model. The preview uses filenames and MIME types, and reports files outside the model
  input budget. Host exposure, approval, durable scheduling, and Settings are still open.
