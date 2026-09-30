import { t } from '../../i18n';

export function sizeLabel(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'] as const;
  const unitIndex = Math.min(Math.floor(Math.log(bytes) / Math.log(1_024)), units.length - 1);
  const value = bytes / 1_024 ** unitIndex;
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)} ${units[unitIndex]}`;
}

export function formatListingSummary(
  fileCount: number,
  folderCount: number,
  totalSize: number,
): string {
  const filesPart = t('pane', 'fileCount', fileCount);
  const foldersPart = t('pane', 'folderCount', folderCount);
  const countsText =
    folderCount === 0
      ? filesPart
      : fileCount === 0
        ? foldersPart
        : t('pane', 'filesAndFolders', { files: filesPart, folders: foldersPart });
  return t('pane', 'sizeIn', { size: sizeLabel(totalSize), counts: countsText });
}
