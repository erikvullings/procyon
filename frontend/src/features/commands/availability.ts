import { t } from '../../i18n';
import type { ActionDescriptor, EntrySummary } from '../../models';
import { archiveRootForEntry } from '../navigation/archive-location';

/** Context the frontend uses to advise which registered commands can run. */
export interface CommandAvailabilityContext {
  readonly selectedEntries: readonly EntrySummary[];
  readonly locationWritable: boolean;
  readonly clipboardHasEntries: boolean;
  readonly openTerminalSupported: boolean;
  /**
   * Whether the current location's provider advertises the `CHECKSUM`
   * capability (spec §6, task 0077). Defaults to `true` so existing callers
   * that predate checksums keep their behaviour.
   */
  readonly checksumSupported?: boolean;
  /** Whether the active pane has a directory to scan for duplicates. */
  readonly hasActiveLocation?: boolean;
}

export interface AvailableAction {
  readonly action: ActionDescriptor;
  readonly available: boolean;
  readonly reason?: string;
}

const LOCATION_ACTION_IDS = new Set([
  'core.createDirectory',
  'core.paste',
  'core.refresh',
  'core.openTerminal',
]);

const SELECTION_ACTION_IDS = new Set([
  'core.open',
  'core.view',
  'core.edit',
  'core.openWith',
  'core.quickLook',
  'core.revealInSystemFileManager',
  'core.copy',
  'core.move',
  'core.rename',
  'core.trash',
  'core.delete',
  'core.copyName',
  'core.copyPath',
  'core.copyRelativePath',
  'core.pack',
  'core.moveToArchive',
  'core.extract',
  'core.editFinderTags',
  'core.editSpotlightComment',
  'core.uninstallApplication',
]);

const CONTEXT_MENU_SELECTION_ORDER = new Map([
  ['core.copyName', 0],
  ['core.copyPath', 1],
  ['core.copyRelativePath', 2],
]);

// `core.trash` is deliberately excluded: unlike rename/move/permanent-delete,
// trashing is reversible and requires no `overrideReadOnly` escape hatch, so
// read-only selected entries stay trashable (task 0043).
const WRITE_SELECTION_ACTION_IDS = new Set(['core.rename', 'core.move', 'core.delete']);

function unavailable(action: ActionDescriptor, reason: string): AvailableAction {
  return { action, available: false, reason };
}

/**
 * Evaluates the registry requirements plus client-only context that the
 * backend re-validates when a command reaches its operation endpoint.
 */
export function evaluateActionAvailability(
  action: ActionDescriptor,
  context: CommandAvailabilityContext,
): AvailableAction {
  const requirements = action.contextRequirements;
  if (requirements.featureAvailable === false)
    return unavailable(action, t('availability', 'notAvailableYet'));
  if (requirements.requiresSingleSelection && context.selectedEntries.length !== 1) {
    return unavailable(action, t('availability', 'selectExactlyOne'));
  }
  if (requirements.requiresSelection && context.selectedEntries.length === 0) {
    return unavailable(action, t('availability', 'selectFirst'));
  }
  const soleSelectedEntry =
    context.selectedEntries.length === 1 ? context.selectedEntries[0] : undefined;
  if (
    action.id === 'core.extract' &&
    soleSelectedEntry !== undefined &&
    archiveRootForEntry(soleSelectedEntry) === undefined
  ) {
    return unavailable(action, t('availability', 'selectArchive'));
  }
  if (action.id === 'core.openTerminal' && !context.openTerminalSupported) {
    return unavailable(action, t('availability', 'terminalUnsupported'));
  }
  if (
    action.id === 'core.uninstallApplication' &&
    (soleSelectedEntry === undefined || !soleSelectedEntry.name.toLowerCase().endsWith('.app'))
  ) {
    return unavailable(action, t('availability', 'selectApplication'));
  }
  if (
    action.id === 'core.quickLook' &&
    (soleSelectedEntry?.kind !== 'file' ||
      soleSelectedEntry.location.providerId !== 'local' ||
      !soleSelectedEntry.location.uri.startsWith('file://'))
  ) {
    return unavailable(action, t('availability', 'quickLookLocalFilesOnly'));
  }
  // Checksums and duplicate detection both stream file contents through the
  // provider, so both need `CHECKSUM` (spec §6, task 0077).
  if (
    (action.id === 'core.calculateChecksum' || action.id === 'core.findDuplicates') &&
    context.checksumSupported === false
  ) {
    return unavailable(action, t('availability', 'checksumsUnsupported'));
  }
  if (
    action.id === 'core.calculateChecksum' &&
    !context.selectedEntries.some((entry) => entry.kind === 'file')
  ) {
    return unavailable(action, t('availability', 'selectFiles'));
  }
  if (action.id === 'core.findDuplicates' && context.hasActiveLocation === false) {
    return unavailable(action, t('availability', 'openDirectoryFirst'));
  }
  if (
    (action.id === 'core.createDirectory' || action.id === 'core.paste') &&
    !context.locationWritable
  ) {
    return unavailable(action, t('availability', 'locationReadOnly'));
  }
  if (action.id === 'core.paste' && !context.clipboardHasEntries) {
    return unavailable(action, t('availability', 'clipboardEmpty'));
  }
  if (
    WRITE_SELECTION_ACTION_IDS.has(action.id) &&
    context.selectedEntries.some((entry) => entry.readOnly)
  ) {
    return unavailable(action, t('availability', 'selectionReadOnly'));
  }
  return { action, available: true };
}

/** Evaluates all registry actions with the same pure predicate. */
export function availableActions(
  actions: readonly ActionDescriptor[],
  context: CommandAvailabilityContext,
): readonly AvailableAction[] {
  return actions.map((action) => evaluateActionAvailability(action, context));
}

/** Selects the registered actions appropriate for a directory-table context menu. */
export function menuActionsForContext(
  actions: readonly ActionDescriptor[],
  context: CommandAvailabilityContext,
): readonly AvailableAction[] {
  const selected = context.selectedEntries.length > 0;
  const matchingActions = actions.filter((action) =>
    selected ? SELECTION_ACTION_IDS.has(action.id) : LOCATION_ACTION_IDS.has(action.id),
  );
  if (selected) {
    matchingActions.sort(
      (left, right) =>
        (CONTEXT_MENU_SELECTION_ORDER.get(left.id) ?? Number.MAX_SAFE_INTEGER) -
        (CONTEXT_MENU_SELECTION_ORDER.get(right.id) ?? Number.MAX_SAFE_INTEGER),
    );
  }
  return availableActions(matchingActions, context);
}
