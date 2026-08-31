import { t } from '../../i18n';

/** Declarative host adapter for the sample.fileAge plugin contribution. */
export const fileAgeColumn = {
  id: 'sample.fileAge',
  get title() {
    return t('table', 'age');
  },
  refreshIntervalMs: 60_000,
  display(modifiedAt: string | undefined, now: number): string {
    if (modifiedAt === undefined) return '';
    const timestamp = Date.parse(modifiedAt);
    if (!Number.isFinite(timestamp)) return '';
    const seconds = Math.max(0, Math.floor((now - timestamp) / 1_000));
    if (seconds < 60) return '0m';
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return `${minutes}m`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h`;
    const days = Math.floor(hours / 24);
    if (days < 30) return `${days}d`;
    const months = Math.floor(days / 30);
    if (months < 12) return `${months}mo`;
    return `${Math.floor(months / 12)}y`;
  },
  compare(left: { readonly modifiedAt?: string }, right: { readonly modifiedAt?: string }): number {
    if (left.modifiedAt === right.modifiedAt) return 0;
    if (left.modifiedAt === undefined) return -1;
    if (right.modifiedAt === undefined) return 1;
    return Date.parse(left.modifiedAt) - Date.parse(right.modifiedAt);
  },
  changedEntryIds(
    entries: readonly { readonly id: string; readonly modifiedAt?: string }[],
    previousNow: number,
    nextNow: number,
  ): readonly string[] {
    return entries
      .filter(
        (entry) =>
          this.display(entry.modifiedAt, previousNow) !== this.display(entry.modifiedAt, nextNow),
      )
      .map((entry) => entry.id);
  },
} as const;
