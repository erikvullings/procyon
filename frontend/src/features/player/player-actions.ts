import type { EnglishCatalogue } from '../../i18n/en';
import type { QueueMode } from './play-selection';

/** Frontend-only `client.*` actions for the audio queue (task 0262), listed in the command
 * palette and bindable in Settings while the queue is non-empty. */
export const PLAYER_CLIENT_ACTIONS: readonly {
  readonly id: string;
  readonly title: keyof EnglishCatalogue['player'];
}[] = [
  { id: 'client.playerTogglePlay', title: 'togglePlay' },
  { id: 'client.playerNext', title: 'next' },
  { id: 'client.playerPrevious', title: 'previous' },
];

export interface MediaQueueAction {
  readonly id: string;
  readonly title: keyof EnglishCatalogue['player'];
  readonly mode: QueueMode;
  readonly recursive: boolean;
  /** Shown in the file context menu; the recursive variants are palette-only. */
  readonly inContextMenu: boolean;
}

/** Play / Add to queue / Play next for the selection (task 0263). */
export const MEDIA_QUEUE_ACTIONS: readonly MediaQueueAction[] = [
  { id: 'client.play', title: 'playAction', mode: 'play', recursive: false, inContextMenu: true },
  {
    id: 'client.playNext',
    title: 'playNextAction',
    mode: 'next',
    recursive: false,
    inContextMenu: true,
  },
  {
    id: 'client.addToQueue',
    title: 'addToQueue',
    mode: 'enqueue',
    recursive: false,
    inContextMenu: true,
  },
  {
    id: 'client.playRecursive',
    title: 'playRecursive',
    mode: 'play',
    recursive: true,
    inContextMenu: false,
  },
  {
    id: 'client.addToQueueRecursive',
    title: 'addToQueueRecursive',
    mode: 'enqueue',
    recursive: true,
    inContextMenu: false,
  },
];

export const MEDIA_QUEUE_ACTION_IDS: ReadonlySet<string> = new Set(
  MEDIA_QUEUE_ACTIONS.map((action) => action.id),
);

/** "Save as playlist…" for the selection, or for the whole collection basket (task 0264). */
export const SAVE_PLAYLIST_ACTION_ID = 'client.savePlaylist';

/** Scans the selected folders (or the active one) for duplicate and near-duplicate music (task 0265). */
export const FIND_MUSIC_DUPLICATES_ACTION_ID = 'client.findMusicDuplicates';

/** Scans folders for broken symlinks, shortcuts, playlist entries and document links (task 0266). */
export const FIND_BROKEN_LINKS_ACTION_ID = 'client.findBrokenLinks';

/** Records the selected (or active) folder into a snapshot file (task 0268). */
export const SAVE_SNAPSHOT_ACTION_ID = 'client.saveSnapshot';
/** Compares a folder against the selected snapshot file (task 0268). */
export const COMPARE_SNAPSHOT_ACTION_ID = 'client.compareWithSnapshot';

/** Acts on the path or URL under the caret of the active pane's editor (task 0271). */
export const OPEN_LINK_AT_CURSOR_ACTION_ID = 'client.openLinkAtCursor';
export const REVEAL_LINK_AT_CURSOR_ACTION_ID = 'client.revealLinkAtCursor';
