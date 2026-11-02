import { CHANGE_ATTRIBUTES_ACTION_ID } from '../features/attributes/attribute-form';
import {
  COMPARE_BY_CONTENT_ACTION_ID,
  COMPARE_FILES_ACTION_ID,
  COMPARE_FILES_SHORTCUT,
} from '../features/file-diff/file-diff-actions';
import { CONVERT_GEO_ACTION_ID } from '../features/geo/convert-geo-actions';
import { MINIFY_GEO_ACTION_ID } from '../features/geo/minify-geo-actions';
import { SHOW_ON_MAP_ACTION_ID } from '../features/image-map/image-map-actions';
import { EDIT_IMAGE_METADATA_ACTION_ID } from '../features/image-metadata/image-metadata-edit';
import {
  COMPARE_SNAPSHOT_ACTION_ID,
  FIND_BROKEN_LINKS_ACTION_ID,
  FIND_MUSIC_DUPLICATES_ACTION_ID,
  MEDIA_QUEUE_ACTIONS,
  OPEN_LINK_AT_CURSOR_ACTION_ID,
  PLAYER_CLIENT_ACTIONS,
  REVEAL_LINK_AT_CURSOR_ACTION_ID,
  SAVE_PLAYLIST_ACTION_ID,
  SAVE_SNAPSHOT_ACTION_ID,
} from '../features/player/player-actions';
import { COMBINE_FILES_ACTION_ID, SPLIT_FILE_ACTION_ID } from '../features/split/split-actions';
import { SYNC_PROFILES_ACTION_ID } from '../features/sync-profiles/sync-profile-actions';
import { actionTitle, t } from '../i18n';
import type { ActionDescriptor } from '../models';

/** Sets up the actionsWiring feature wiring and returns its functions. */
import type { ShellState } from './shell-state';

export function createActionsWiring(shellState: ShellState) {
  function favouriteActions(): readonly ActionDescriptor[] {
    const favourites = shellState.currentSettings?.favouriteLocations ?? [];
    return [
      {
        id: 'core.favourites',
        title: t('action', 'openFavourites'),
        description: t('action', 'showSavedLocations'),
        category: 'navigation',
        defaultShortcuts: [{ key: 'h', ctrl: true, shift: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      ...favourites.map((favourite, index) => ({
        id: `core.favourite.${index}`,
        title: t('action', 'openFavourite', { name: favourite.label }),
        description: favourite.location.uri,
        category: 'navigation',
        // TC-style quick-switch-to-saved-location: Ctrl/Cmd+1..9 for the first nine favourites
        // (task 0129's Alt+F1/Alt+F2 "switch panel to a different drive" row — fm has no drive
        // concept, but jumping to a saved favourite location is the closest equivalent).
        defaultShortcuts: index < 9 ? [{ key: String(index + 1), ctrl: true }] : [],
        contextRequirements: {},
        source: { kind: 'core' as const },
      })),
    ];
  }

  function mediaQueueActions(): readonly ActionDescriptor[] {
    const descriptor = (
      id: string,
      title: string,
      contextRequirements: ActionDescriptor['contextRequirements'],
      description?: string,
    ): ActionDescriptor => ({
      id,
      title,
      ...(description === undefined ? {} : { description }),
      category: 'tools',
      defaultShortcuts: [],
      contextRequirements,
      source: { kind: 'core' },
    });
    return [
      ...MEDIA_QUEUE_ACTIONS.map(({ id, title }) =>
        descriptor(id, t('player', title), { requiresSelection: true }),
      ),
      descriptor(SAVE_PLAYLIST_ACTION_ID, t('playlistExport', 'action'), {}),
      descriptor(EDIT_IMAGE_METADATA_ACTION_ID, t('imageEdit', 'action'), {
        requiresSelection: true,
      }),
      descriptor(CHANGE_ATTRIBUTES_ACTION_ID, t('attributes', 'action'), {
        requiresSelection: true,
      }),
      descriptor(
        SHOW_ON_MAP_ACTION_ID,
        t('imageMap', 'action'),
        {},
        t('imageMap', 'actionDescription'),
      ),
      {
        // Shift+F2 is taken by core.compareDirectories (Total Commander's own binding) and
        // Ctrl+F1..F7 by view/sort; Alt+Shift+F2 is the closest free Shift+F2-family chord.
        ...descriptor(COMPARE_FILES_ACTION_ID, t('fileDiff', 'compareAction'), {
          requiresSelection: true,
        }),
        defaultShortcuts: [COMPARE_FILES_SHORTCUT],
      },
      descriptor(COMPARE_BY_CONTENT_ACTION_ID, t('fileDiff', 'compareByContentAction'), {}),
      descriptor(SYNC_PROFILES_ACTION_ID, t('syncProfiles', 'action'), {}),
      descriptor(SPLIT_FILE_ACTION_ID, t('splitCombine', 'splitAction'), {
        requiresSelection: true,
      }),
      descriptor(COMBINE_FILES_ACTION_ID, t('splitCombine', 'combineAction'), {
        requiresSelection: true,
      }),
      descriptor(CONVERT_GEO_ACTION_ID, t('geoConvert', 'action'), { requiresSelection: true }),
      descriptor(MINIFY_GEO_ACTION_ID, t('geoMinify', 'action'), { requiresSelection: true }),
    ];
  }

  function localisedRegisteredActions(): readonly ActionDescriptor[] {
    return shellState.registeredActions.map((action) => ({
      ...action,
      title: actionTitle(action.id, action.title),
    }));
  }

  function keybindingActions(): readonly ActionDescriptor[] {
    const actions = localisedRegisteredActions();
    if (actions.some((action) => action.id === 'core.showShortcutsHelp')) return actions;
    return [
      ...actions,
      {
        id: 'core.showShortcutsHelp',
        title: t('keybindingsHelp', 'title'),
        category: 'navigation',
        defaultShortcuts: [{ key: 'F1' }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ];
  }

  function clientOnlyActions(): readonly ActionDescriptor[] {
    const hasQueue = (shellState.appState?.player.queue.length ?? 0) > 0;
    const editorOpen =
      shellState.workspace !== undefined &&
      shellState.editorByPane.has(shellState.workspace.activePaneId);
    return [
      ...(editorOpen
        ? [
            {
              id: OPEN_LINK_AT_CURSOR_ACTION_ID,
              title: t('editorLinks', 'openAtCursor'),
              category: 'tools',
              defaultShortcuts: [],
              contextRequirements: {},
              source: { kind: 'core' as const },
            },
            {
              id: REVEAL_LINK_AT_CURSOR_ACTION_ID,
              title: t('editorLinks', 'revealAtCursor'),
              category: 'tools',
              defaultShortcuts: [],
              contextRequirements: {},
              source: { kind: 'core' as const },
            },
          ]
        : []),
      ...mediaQueueActions(),
      {
        id: SAVE_SNAPSHOT_ACTION_ID,
        title: t('snapshots', 'saveAction'),
        category: 'tools',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' as const },
      },
      {
        id: COMPARE_SNAPSHOT_ACTION_ID,
        title: t('snapshots', 'compareAction'),
        category: 'tools',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' as const },
      },
      {
        id: FIND_BROKEN_LINKS_ACTION_ID,
        title: t('linkDoctor', 'action'),
        category: 'tools',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' as const },
      },
      {
        id: FIND_MUSIC_DUPLICATES_ACTION_ID,
        title: t('musicDuplicates', 'action'),
        category: 'tools',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' as const },
      },
      ...(hasQueue
        ? PLAYER_CLIENT_ACTIONS.map(({ id, title }) => ({
            id,
            title: t('player', title),
            category: 'tools',
            defaultShortcuts: [],
            contextRequirements: {},
            source: { kind: 'core' as const },
          }))
        : []),
      {
        id: 'client.toggleDirectoryTree',
        title: t('tree', 'toggleSidebar'),
        description: t('tree', 'directoryTree'),
        category: 'navigation',
        defaultShortcuts: [{ key: 'F10', alt: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'client.toggleOperationCentre',
        title:
          shellState.workspace?.operationCentre.visible === true
            ? t('shell', 'hideOperationCentre')
            : t('shell', 'showOperationCentre'),
        category: 'navigation',
        defaultShortcuts: [{ key: 'z', alt: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'client.diskUsage',
        title: t('diskUsage', 'tabTitle'),
        description: t('diskUsage', 'description'),
        category: 'tools',
        defaultShortcuts: [{ key: 'l', ctrl: true, shift: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      ...(shellState.knowledgeSearchAvailable
        ? [
            {
              id: 'client.searchKnowledge',
              title: t('knowledgeSearch', 'openTitle'),
              description: t('knowledgeSearch', 'openDescription'),
              category: 'tools',
              defaultShortcuts: [{ key: 'k', ctrl: true, shift: true }],
              contextRequirements: {},
              source: { kind: 'core' as const },
            },
          ]
        : []),
      ...(shellState.themesAvailable
        ? [
            {
              id: 'client.exploreThemes',
              title: t('themes', 'openTitle'),
              description: t('themes', 'openDescription'),
              category: 'tools',
              defaultShortcuts: [],
              contextRequirements: { featureAvailable: shellState.themesWorkspaceIndexed },
              source: { kind: 'core' as const },
            },
          ]
        : []),
      ...(shellState.semanticAssistantAvailable
        ? [
            {
              id: 'client.semanticAssistant',
              title: t('ragAsk', 'title'),
              description: t('ragAsk', 'openAssistant'),
              category: 'tools',
              defaultShortcuts: [{ key: 'f', ctrl: true, shift: true }],
              contextRequirements: {},
              source: { kind: 'core' as const },
            },
          ]
        : []),
    ];
  }

  function actionsWithFavourites(): readonly ActionDescriptor[] {
    return [...localisedRegisteredActions(), ...clientOnlyActions(), ...favouriteActions()];
  }

  return {
    favouriteActions,
    mediaQueueActions,
    localisedRegisteredActions,
    keybindingActions,
    clientOnlyActions,
    actionsWithFavourites,
  };
}
