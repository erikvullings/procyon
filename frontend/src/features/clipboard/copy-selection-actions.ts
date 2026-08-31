import { t } from '../../i18n';
import type { EntrySummary, Location } from '../../models';
import { pathFromUri } from '../workspace/workspace-layout';

/** Core actions that copy a textual representation of the selected entries. */
export type CopySelectionActionId = 'core.copyName' | 'core.copyPath' | 'core.copyRelativePath';

/** Narrows an action id to a frontend-owned selection clipboard action. */
export function isCopySelectionAction(actionId: string): actionId is CopySelectionActionId {
  return (
    actionId === 'core.copyName' ||
    actionId === 'core.copyPath' ||
    actionId === 'core.copyRelativePath'
  );
}

function relativePath(from: string, to: string): string {
  const fromSegments = from.split('/').filter(Boolean);
  const toSegments = to.split('/').filter(Boolean);
  let sharedSegments = 0;
  while (
    sharedSegments < fromSegments.length &&
    fromSegments[sharedSegments] === toSegments[sharedSegments]
  ) {
    sharedSegments += 1;
  }
  return [
    ...Array.from({ length: fromSegments.length - sharedSegments }, () => '..'),
    ...toSegments.slice(sharedSegments),
  ].join('/');
}

/** Produces the text copied by a selection path/name action, one entry per line. */
export function selectionClipboardText(
  actionId: CopySelectionActionId,
  selectedEntries: readonly EntrySummary[],
  activeDirectory: Location,
): string | undefined {
  if (selectedEntries.length === 0) return undefined;
  return selectedEntries
    .map((entry) => {
      if (actionId === 'core.copyName') return entry.name;
      const path = pathFromUri(entry.location.uri);
      return actionId === 'core.copyPath'
        ? path
        : relativePath(pathFromUri(activeDirectory.uri), path);
    })
    .join('\n');
}

/** Writes text to the host clipboard, including WebViews without the modern Clipboard API. */
export async function writeSystemClipboardText(text: string): Promise<void> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText !== undefined) {
    await navigator.clipboard.writeText(text);
    return;
  }
  if (typeof document === 'undefined') {
    throw new Error(t('clipboard', 'unavailable'));
  }
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.append(textarea);
  textarea.select();
  const copied = document.execCommand('copy');
  textarea.remove();
  if (!copied) throw new Error(t('clipboard', 'writeFailed'));
}

/** Formats and copies the current selection, returning false when it is empty. */
export async function copySelectionToClipboard(
  actionId: CopySelectionActionId,
  selectedEntries: readonly EntrySummary[],
  activeDirectory: Location,
  writeText: (text: string) => Promise<void> = writeSystemClipboardText,
): Promise<boolean> {
  const text = selectionClipboardText(actionId, selectedEntries, activeDirectory);
  if (text === undefined) return false;
  await writeText(text);
  return true;
}
