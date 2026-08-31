# 0008 Virtualized table implementation

Status: accepted

## Context
Directory listings can contain very large numbers of entries, and the spec forbids rendering large
directories without virtualization (spec §35; §24 directory table).

## Decision
The directory table is built as a custom virtualized list component within the existing Mithril
component model: it renders only the rows within (and a small buffer around) the visible
scroll viewport, computing row height and offsets from a fixed or measured row size, and re-renders
the visible window on scroll rather than mounting a DOM node per entry.

## Alternatives
- **Render every row, rely on browser performance**: rejected — spec §35 explicitly forbids this for
  large directories; real directories can have tens of thousands of entries.
- **Adopt a third-party virtualization library**: considered, but most mature options are
  React-specific; a Mithril-native implementation avoids a cross-framework adapter layer and keeps
  the component small enough to own directly given the app's specific needs (fixed columns, known
  row shape).
- **Pagination instead of virtualization**: rejected — breaks the "scroll a folder like a normal file
  manager" interaction model and complicates selection/keyboard navigation across pages.

## Consequences
- The table component owns scroll-position-to-row-range math and must stay correct as rows are
  inserted/removed (e.g. from filesystem watch deltas), or the visible window can desync from the
  data.
- Row height assumptions (fixed vs. variable) constrain what a row can render; variable-height rows
  would need a more expensive measurement pass.
- Keyboard navigation and selection (tasks 0028/0029) must operate on data indices, not DOM nodes,
  since off-screen rows don't exist in the DOM.

## Revisit conditions
Revisit if rows need variable height (e.g. wrapping long names or multi-line metadata) that the
fixed-row-height assumption can't support, or if scroll performance degrades on very large
directories despite virtualization, indicating the windowing strategy itself needs to change.
