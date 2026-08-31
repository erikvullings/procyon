import { t } from '../../i18n';
import type { EntryId, EntrySummary } from '../../models';

/** Mirrors `panes/parent-entry.ts::isParentEntry` (not imported directly to avoid a
 * directory-table -> panes dependency edge). */
const PARENT_ENTRY_PREFIX = 'fm:parent:';

function isParentEntryId(id: EntryId | undefined): boolean {
  return id?.startsWith(PARENT_ENTRY_PREFIX) ?? false;
}

/** A contiguous run of entries sharing the same calendar day (task 0134's "photo app mode"). Runs
 * are contiguous in the *given* order, not re-sorted here - the grid still renders entries in
 * whatever order the pane's active sort produced, so a date-descending sort yields one group per
 * day while any other sort may legitimately split a day into several runs. */
export interface DayGroup {
  readonly dayKey: string;
  readonly label: string;
  readonly startIndex: number;
  readonly count: number;
}

const dayLabelFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'full' });

function dayKeyAndLabel(modifiedAt: string | undefined): { dayKey: string; label: string } {
  if (modifiedAt === undefined)
    return { dayKey: 'unknown', label: t('photoGrouping', 'unknownDate') };
  const date = new Date(modifiedAt);
  if (Number.isNaN(date.getTime()))
    return { dayKey: 'unknown', label: t('photoGrouping', 'unknownDate') };
  return { dayKey: modifiedAt.slice(0, 10), label: dayLabelFormatter.format(date) };
}

/** Groups entries into contiguous same-day runs, preserving the given order. */
export function groupEntriesByDay(entries: readonly EntrySummary[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (let index = 0; index < entries.length; index += 1) {
    const entry = entries[index];
    if (entry === undefined) continue;
    const { dayKey, label } = dayKeyAndLabel(entry.modifiedAt);
    const current = groups.at(-1);
    if (current !== undefined && current.dayKey === dayKey) {
      groups[groups.length - 1] = { ...current, count: current.count + 1 };
    } else {
      groups.push({ dayKey, label, startIndex: index, count: 1 });
    }
  }
  return groups;
}

/** One rendered line in photo-mode layout: a full-width day header, or a row of up to
 * `columnsPerRow` tiles starting at `startIndex` within the original entry order. */
export type GridLine =
  | { readonly kind: 'header'; readonly label: string }
  | { readonly kind: 'row'; readonly startIndex: number; readonly count: number };

/** Expands day groups into the header/row line sequence a virtualized grid can window over. The
 * presentation-only parent-directory row (`..`) is rendered ungrouped, ahead of any day header -
 * it isn't a real file and grouping it under "Unknown date" reads as a bogus title. */
export function layoutPhotoLines(
  entries: readonly EntrySummary[],
  columnsPerRow: number,
): GridLine[] {
  const lines: GridLine[] = [];
  let parentCount = 0;
  while (parentCount < entries.length && isParentEntryId(entries[parentCount]?.id)) {
    parentCount += 1;
  }
  for (let offset = 0; offset < parentCount; offset += columnsPerRow) {
    lines.push({
      kind: 'row',
      startIndex: offset,
      count: Math.min(columnsPerRow, parentCount - offset),
    });
  }
  for (const group of groupEntriesByDay(entries.slice(parentCount))) {
    lines.push({ kind: 'header', label: group.label });
    for (let offset = 0; offset < group.count; offset += columnsPerRow) {
      lines.push({
        kind: 'row',
        startIndex: parentCount + group.startIndex + offset,
        count: Math.min(columnsPerRow, group.count - offset),
      });
    }
  }
  return lines;
}
