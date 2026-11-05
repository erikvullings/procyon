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
- Rules evaluate on new-file events only; idempotent so a re-arrival or restart doesn't double-act.
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
- (none yet)
