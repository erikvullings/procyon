# 0313 More compelling timeline / date view

Status: open
Priority: medium
Subsystem: frontend
Depends on: 0134

## Context
Task 0134's "photo app mode" already groups grid tiles into **same-day runs** via
`photo-grouping.ts` (`groupEntriesByDay` / `layoutPhotoLines`), keyed on `modifiedAt`. It works but
is limited: it only buckets by *day*, the runs are contiguous-in-current-sort-order (so any non
date-descending sort legitimately splits a single day into several runs), and it's a grid overlay
rather than a real *timeline* you navigate. Users browsing photo/document archives want to jump by
week/month/year and move through time, not just see day headers above a tile grid.

This is an enhancement to 0134's grouping, not a new subsystem — it reuses the date bucketing and
the virtualized grid.

## Acceptance Criteria
- **Granularity toggle**: group by day / week / month / year, selectable (persisted per tab, 0030);
  buckets are computed from the entry date regardless of the active sort, so a month is one bucket
  even if the pane is sorted by name.
- **Choose the date axis**: `modifiedAt` by default, with an opt-in switch to the photo EXIF
  "date taken" (when available) / `createdAt` so a camera roll groups by when a shot was made, not
  when it landed on disk.
- **Navigation**: a lightweight way to jump within the timeline — scroll-to-next-bucket control and
  a minimap/calendar-strip (month grid) that highlights the current bucket and jumps on click.
- **Virtualized**: the timeline is windowed like the rest of the grid (0024) — grouping a large
  folder produces one header per occupied bucket, not per calendar unit, and rendering stays bounded.
- **Photo-mode parity**: works in both grid and any future list presentation; keyboard-accessible
  (0066); both browser and Tauri hosts behave identically.

## Implementation Notes
- Extend `groupEntriesByDay` into a general `bucketEntriesByDate(entries, granularity, axis)` that
  returns stable bucket keys; keep `layoutPhotoLines`'s header/row `GridLine` shape so the grid
  renderer barely changes.
- EXIF date extraction should reuse the existing image-metadata path (0273) rather than add a new
  parser; fall back to `modifiedAt` when no EXIF date is present.
- The calendar-strip minimap is presentational and can be a small standalone component; it only needs
  the set of occupied bucket keys and the current one.
- Don't remove today's day-grouping behaviour — the granularity toggle is additive, with "day" as
  the current default so nothing regresses for existing photo-mode users.

## Agent Notes
- (none yet)
