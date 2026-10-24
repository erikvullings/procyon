# 0161 Saved synchronization profiles

Status: done
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
- 2026-10-24 Claude: Implemented end to end; profiles are saved input to the existing comparison and
  sync-plan services, not a second sync implementation.
  - Decisions: profiles and schedules live in **global settings** (`Settings.sync_profiles`), not a
    workspace. Scheduled execution requires the Procyon window (desktop app, or the browser UI
    against `fm-server`) to be open: the shell checks every 60 s for due profiles; there is no
    background daemon. A schedule is off until explicitly enabled; its default policy is `required`
    (a person reviews every plan). The only unattended alternative, `autoApplyAdditions`, applies
    only copies onto a side that lacks the entry; overwrites and deletions always wait for review,
    whatever the conflict default says.
  - Backend: `fm-comparison::filter` (`PathFilter`, `generate_filtered_sync_plan`: `*`/`**`/`?`
    globs, segment vs. rooted patterns, ancestors match, exclusions win). `GenerateSyncPlanRequestDto`
    gains `include`/`exclude`, `ApplySyncPlanRequestDto` gains optional `conflictPolicy` (default
    still `overwrite`); `SearchComparisonCoordinator` uses them, the facade is untouched. Settings:
    `fm-settings::sync_profile` (`SyncProfile`, `SyncSchedule`, `SyncRunSummary`), no schema bump
    (`#[serde(default)]`; v1/v4/v8/current load empty), sanitized on load and save: location URIs
    lose userinfo passwords, queries and fragments, text/pattern/profile counts are bounded, ids
    deduplicated, schedule interval clamped to 5 min .. 7 days. DTOs + `sync_profile_mapping.rs`;
    OpenAPI and Orval client regenerated.
  - Frontend (`features/sync-profiles/`): pure `sync-profile.ts` (defaults: two-way, ask; validation,
    duplicate, due-schedule, addition-only split, staleness), `sync-profile-runner.ts` (preflight
    listing of both roots, fresh comparison, poll to completion, filtered dry-run plan; apply with
    stale-plan and unknown-plan guards), `sync-profile-controller.ts` (persistence through
    `updateLocationSettings`, last-run summaries, scheduled tick, one pending review at a time),
    `sync-profiles-dialog.ts` (list, editor, run, duplicate, two-step delete) and the existing
    `SyncPlanDialog` for review. Action `client.syncProfiles` ("Synchronization profiles…").
    HTTP/Tauri clients pass filters and conflict policy; the mock mirrors the filter
    (`sync-path-filter.ts`). The settings editor save preserves `syncProfiles`. i18n for 8 locales.
  - Errors (actionable, name the side): missing connection, unavailable location, destination now
    read-only (capability change; only checked for sides the direction writes), comparison timeout,
    plan older than 15 min, plan no longer held by the backend, apply failure.
  - Verified: Rust `fm-comparison` 5 new filter tests, `fm-settings` 4 new tests (migration of older
    files, credential scrubbing, sanitization, bound), `fm-application` 1 mapping test and 2 new
    integration tests in `comparison_and_sync.rs` (filters narrow plan and applied changes on real
    temp dirs; skip policy keeps a differing destination); frontend 13 `sync-profile.test.ts`, 8
    `sync-profile-runner.test.ts`, 10 `sync-profile-controller.test.ts`, 3 dialog tests, 4
    `sync-path-filter.test.ts`, 1 mock filter test, 1 settings-mapping test, 2 added client tests
    (HTTP, Tauri).
  - Known gaps: no OS-level scheduler (schedules stop when the app is closed); local-vs-remote pairs
    are exercised through scripted/mock clients and real local temp directories, not a live
    SFTP/FTP/WebDAV server; the dialog has no free-text location entry (roots come from the open
    panes); filters apply to compared results, they do not prune the traversal; no dedicated keyboard
    shortcut or toolbar button (command palette/action only).
