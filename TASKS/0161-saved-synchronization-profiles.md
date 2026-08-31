# 0161 Saved synchronization profiles

Status: open
Priority: high
Subsystem: backend, frontend, comparison
Depends on: 0030, 0075

## Context

Directory comparison and synchronization already produce and execute sync plans, but users must
configure each recurring pair again. Named profiles should retain the two roots, direction, filters,
comparison options, and conflict policy while preserving the existing mandatory preview.

## Acceptance Criteria

- Users can create, edit, duplicate, delete, and manually run named synchronization profiles.
- A profile stores provider-neutral source and destination locations, direction, inclusion/exclusion
  filters, comparison mode, and conflict defaults without storing credentials.
- Running a profile always refreshes both roots and shows a dry-run plan before any mutation.
- Missing connections, unavailable locations, changed provider capabilities, and stale plans produce
  actionable errors rather than silently changing profile behavior.
- Optional schedules can be enabled explicitly; scheduled runs still create an inspectable plan and
  obey a configured policy for whether human confirmation is required.
- Profile changes and last-run summaries persist through the existing versioned settings or workspace
  repository, with HTTP/Tauri parity.
- Tests cover persistence migration, local and remote pairs, stale plans, filters, scheduled-run
  confirmation policy, and credential redaction.

## Implementation Notes

- Reuse `fm-comparison` and the current synchronization operation path. A profile is saved input to
  those services, not a second sync implementation.
- Decide explicitly whether a schedule belongs to global settings or a workspace. Scheduled execution
  must state whether it requires the desktop/backend to be running.
- Never default an unattended run to destructive conflict resolution.

## Agent Notes

- 2026-08-28: Created from the product feature review. Manual profiles are the core feature;
  scheduling should be designed as an explicit, safe extension rather than hidden timer behavior.
