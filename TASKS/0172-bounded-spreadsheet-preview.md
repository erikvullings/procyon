# 0172 Bounded spreadsheet preview

Status: open
Priority: medium
Subsystem: backend, frontend
Depends on: 0100

## Context

Task 0100 already routes `.xlsx`, `.xlsb`, and `.xls` through the provider-neutral structured-view
API, but deliberately returns `externalFallback`: the available worksheet APIs materialize sheets
and cannot honestly promise constant memory for arbitrarily large workbooks. Replace that fallback
for workbooks inside an explicit budget while retaining it for unsafe inputs.

Use `calamine` as the MIT/Apache-licensed native Rust reader and reuse the existing virtualized
structured table. VisiGrid demonstrates a much richer Rust spreadsheet implementation, but its
internal `visigrid-io` crate is not a stable standalone library and is AGPL-3.0, which is
incompatible with this MIT application's built-in implementation without separate commercial
licensing. Do not copy or link VisiGrid core code.

## Acceptance Criteria

- F3 opens supported `.xlsx`, `.xlsb`, and `.xls` workbooks inside the existing structured viewer
  when they fit documented source, workbook, sheet, row, column, cell, string, and image budgets.
- The existing external-application fallback remains available and is selected before or during
  parsing whenever the viewer cannot stay within its budget; the UI explains which limit was hit.
- Workbook preview exposes sheet tabs and presents the selected sheet through the existing
  row-and-column-virtualized grid with sticky headers.
- Cell values preserve useful types, including text, numbers, booleans, errors, and dates/times.
  Formula source and cached/displayed values are exposed where Calamine can provide them, without
  claiming that formulas have been recalculated.
- Empty/sparse ranges do not allocate or transport a dense rectangle proportional to a maliciously
  inflated used range. Pages sent to the frontend remain bounded.
- Source revision changes invalidate the session; switching sheets, reading pages, cancellation,
  and close release all application-owned state.
- HTTP, Tauri, and mock clients expose equivalent behavior through `FileManagerClient`; adapters
  remain thin and no local path enters the application contract.
- Sorting and search follow 0100's established limits and messaging. No workbook editing or saving
  is introduced.
- Tests cover multi-sheet workbooks, sparse ranges, formulas versus cached values, dates, errors,
  merged cells if surfaced, corrupt files, adversarial dimensions/strings, every budget boundary,
  cancellation, revision invalidation, and host parity.

## Implementation Notes

- Extend `crates/fm-application/src/structured_view.rs`; do not create a separate host-specific
  spreadsheet path. The current `StructuredViewFormatDto::Excel` and
  `StructuredViewKindDto::ExternalFallback` are the intended seam.
- Calamine's `worksheet_range` materializes a range; it is not a streaming row-range API. Measure
  peak memory with generated fixtures and set conservative limits before enabling in-app preview.
- For ZIP-based XLSX, preflight archive entry count and expanded sizes before XML parsing. Legacy
  XLS/XLSB require source-size and parsed-workbook limits appropriate to their containers.
- Start with readable values and basic layout. Rich Excel fidelity (complete styles, charts,
  conditional formatting, macros, pivot tables, and formula evaluation) is explicitly out of scope.
- Consider `.ods` only after the Excel formats meet their limits and tests; do not expand the task
  merely because Calamine can parse it.

## Agent Notes

- 2026-08-29: Created as the implementation follow-up to 0100's honest Excel fallback. Calamine is
  recommended for the built-in MIT path. VisiGrid 0.29 is useful reference material and an external
  application target, but its AGPL core must not be embedded or copied into Procyon.
