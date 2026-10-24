import { t } from '../../i18n';
import type { ActionDescriptor, EntrySummary, PluginDescriptor } from '../../models';
import { CONVERT_GEO_ACTION_ID, isConvertibleGeoFile } from '../geo/convert-geo-actions';
import { MINIFY_GEO_ACTION_ID } from '../geo/minify-geo-actions';
import { isGeoCandidate } from '../image-map/gps-scan';
import { SHOW_ON_MAP_ACTION_ID } from '../image-map/image-map-actions';
import {
  EDIT_IMAGE_METADATA_ACTION_ID,
  isMetadataEditable,
} from '../image-metadata/image-metadata-edit';
import { archiveRootForEntry } from '../navigation/archive-location';
import { isParentEntry } from '../panes/parent-entry';
import { hasPlayableSelection } from '../player/play-selection';
import {
  MEDIA_QUEUE_ACTION_IDS,
  MEDIA_QUEUE_ACTIONS,
  SAVE_PLAYLIST_ACTION_ID,
} from '../player/player-actions';
import {
  COMBINE_FILES_ACTION_ID,
  isCombinablePart,
  isSplittable,
  SPLIT_FILE_ACTION_ID,
} from '../split/split-actions';

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
  readonly cursorEntry?: EntrySummary;
  readonly plugins?: readonly PluginDescriptor[];
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
  'core.createLink',
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
  'core.documentSummary',
  ...MEDIA_QUEUE_ACTIONS.filter((action) => action.inContextMenu).map((action) => action.id),
  SAVE_PLAYLIST_ACTION_ID,
  EDIT_IMAGE_METADATA_ACTION_ID,
  SHOW_ON_MAP_ACTION_ID,
  CONVERT_GEO_ACTION_ID,
  MINIFY_GEO_ACTION_ID,
  SPLIT_FILE_ACTION_ID,
  COMBINE_FILES_ACTION_ID,
]);

/**
 * Context-menu sections for a selection: open/inspect, clipboard, organise, archive, then the
 * removal actions last so destructive items are never sandwiched between routine ones.
 */
const CONTEXT_MENU_SELECTION_GROUPS: readonly (readonly string[])[] = [
  [
    'core.open',
    'core.view',
    'core.edit',
    'core.openWith',
    'core.quickLook',
    'core.revealInSystemFileManager',
  ],
  ['client.play', 'client.playNext', 'client.addToQueue', SAVE_PLAYLIST_ACTION_ID],
  ['core.copyName', 'core.copyPath', 'core.copyRelativePath'],
  [
    'core.rename',
    'core.copy',
    'core.move',
    'core.createLink',
    'core.editFinderTags',
    'core.editSpotlightComment',
    'core.documentSummary',
  ],
  ['core.pack', 'core.moveToArchive', 'core.extract'],
  [
    SHOW_ON_MAP_ACTION_ID,
    EDIT_IMAGE_METADATA_ACTION_ID,
    CONVERT_GEO_ACTION_ID,
    MINIFY_GEO_ACTION_ID,
  ],
  [SPLIT_FILE_ACTION_ID, COMBINE_FILES_ACTION_ID],
  ['core.trash', 'core.delete', 'core.uninstallApplication'],
];

const CONTEXT_MENU_SELECTION_ORDER = new Map(
  CONTEXT_MENU_SELECTION_GROUPS.flat().map((actionId, index) => [actionId, index] as const),
);

/** Section index used to separate context-menu groups; unknown actions share a final section. */
export function contextMenuGroup(actionId: string): number {
  const group = CONTEXT_MENU_SELECTION_GROUPS.findIndex((ids) => ids.includes(actionId));
  return group < 0 ? CONTEXT_MENU_SELECTION_GROUPS.length : group;
}

/** Actions whose effect cannot be undone from Trash; rendered with danger emphasis. */
export const DESTRUCTIVE_CONTEXT_ACTION_IDS: ReadonlySet<string> = new Set([
  'core.delete',
  'core.uninstallApplication',
]);

const PARENT_ENTRY_ACTION_IDS = new Set(['core.copyPath', 'core.copyRelativePath']);

// `core.trash` is deliberately excluded: unlike rename/move/permanent-delete,
// trashing is reversible and requires no `overrideReadOnly` escape hatch, so
// read-only selected entries stay trashable (task 0043).
const WRITE_SELECTION_ACTION_IDS = new Set(['core.rename', 'core.move', 'core.delete']);

function unavailable(action: ActionDescriptor, reason: string): AvailableAction {
  return { action, available: false, reason };
}

export function svgoRequiresLocalFile(pluginId: string, entry: EntrySummary | undefined): boolean {
  return (
    pluginId === 'procyon.svgo' &&
    entry !== undefined &&
    (entry.location.providerId !== 'local' || !entry.location.uri.startsWith('file://'))
  );
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
  const panelPlugin = context.plugins?.find((plugin) => plugin.spaPanel?.actionId === action.id);
  if (panelPlugin?.spaPanel !== undefined) {
    const panel = panelPlugin.spaPanel;
    const entry = context.cursorEntry;
    if (svgoRequiresLocalFile(panelPlugin.id, entry))
      return unavailable(action, t('availability', 'svgoLocalFilesOnly'));
    const extension = entry?.name.split('.').at(-1)?.toLowerCase();
    if (
      !panelPlugin.enabled ||
      entry?.kind !== 'file' ||
      extension === undefined ||
      (panel.extensions.length > 0 && !panel.extensions.includes(extension))
    )
      return unavailable(action, t('availability', 'selectFiles'));
    return { action, available: true };
  }
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
  if (action.id === 'core.documentSummary' && soleSelectedEntry?.kind !== 'file') {
    return unavailable(action, t('availability', 'selectFiles'));
  }
  if (
    (MEDIA_QUEUE_ACTION_IDS.has(action.id) ||
      (action.id === SAVE_PLAYLIST_ACTION_ID && context.selectedEntries.length > 0)) &&
    !hasPlayableSelection(context.selectedEntries)
  ) {
    return unavailable(action, t('availability', 'selectFiles'));
  }
  if (
    action.id === SHOW_ON_MAP_ACTION_ID &&
    context.selectedEntries.length > 0 &&
    !context.selectedEntries.some((entry) => entry.kind === 'directory' || isGeoCandidate(entry))
  ) {
    return unavailable(action, t('availability', 'selectFiles'));
  }
  if (
    (action.id === CONVERT_GEO_ACTION_ID || action.id === MINIFY_GEO_ACTION_ID) &&
    !context.selectedEntries.some(isConvertibleGeoFile)
  ) {
    return unavailable(action, t('availability', 'selectFiles'));
  }
  if (
    action.id === SPLIT_FILE_ACTION_ID &&
    context.selectedEntries.length > 0 &&
    !context.selectedEntries.some(isSplittable)
  ) {
    return unavailable(action, t('availability', 'selectFiles'));
  }
  if (
    action.id === COMBINE_FILES_ACTION_ID &&
    context.selectedEntries.length > 0 &&
    !context.selectedEntries.some(isCombinablePart)
  ) {
    return unavailable(action, t('splitCombine', 'noParts'));
  }
  if (
    action.id === EDIT_IMAGE_METADATA_ACTION_ID &&
    !context.selectedEntries.some(isMetadataEditable)
  ) {
    return unavailable(action, t('imageEdit', 'noEditableImages'));
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
  if (context.selectedEntries.length === 1 && isParentEntry(context.selectedEntries[0]?.id)) {
    return availableActions(
      actions.filter((action) => PARENT_ENTRY_ACTION_IDS.has(action.id)),
      context,
    );
  }
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
