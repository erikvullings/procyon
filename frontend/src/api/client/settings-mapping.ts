import { DEFAULT_MAP_SETTINGS } from '../../features/map/map-settings';
import type { Location as FileLocation, SearchQuery, Settings } from '../../models';
import type { FilterCriteria, SyncProfile } from '../../models/settings';
import type { FilterCriteriaDto } from '../generated/models/filterCriteriaDto';
import type { SettingsDto } from '../generated/models/settingsDto';
import type { SyncProfileDto } from '../generated/models/syncProfileDto';

export function settingsFromDto(settings: SettingsDto): Settings {
  return {
    ...settings,
    multiRenamePresets: settings.multiRenamePresets.map((preset) => ({
      ...preset,
      rules: { ...preset.rules, sequence: { ...preset.rules.sequence } },
    })),
    filterPresets: (settings.filterPresets ?? []).map((preset) => ({
      ...preset,
      criteria: filterCriteriaFromDto(preset.criteria),
    })),
    syncProfiles: (settings.syncProfiles ?? []).map(syncProfileFromDto),
    savedSearches: settings.savedSearches.map((saved) => ({
      ...saved,
      query: searchQueryFromDto(saved.query),
    })),
    favouriteLocations: settings.favouriteLocations.map((favourite) => ({
      ...favourite,
      location: { ...favourite.location },
    })),
    recentLocationsByWorkspace: Object.fromEntries(
      Object.entries(settings.recentLocationsByWorkspace).map(([workspaceId, locations]) => [
        workspaceId,
        Array.isArray(locations)
          ? locations.map(
              (location): FileLocation => ({
                providerId: String((location as { providerId?: unknown }).providerId),
                uri: String((location as { uri?: unknown }).uri),
              }),
            )
          : [],
      ]),
    ),
    terminalCommand: settings.terminalCommand ?? null,
    editorCommand: settings.editorCommand ?? null,
    rememberFolderViews: settings.rememberFolderViews ?? true,
    checkForBetaUpdates: settings.checkForBetaUpdates ?? false,
    map: settings.map ?? DEFAULT_MAP_SETTINGS,
  };
}

function searchQueryFromDto(query: SettingsDto['savedSearches'][number]['query']): SearchQuery {
  return {
    schemaVersion: query.schemaVersion === 1 ? 1 : query.schemaVersion === 3 ? 3 : 2,
    mode: query.schemaVersion === 1 && query.content != null ? 'content' : (query.mode ?? 'name'),
    scope: {
      ...query.scope,
      locations: query.scope.locations.map((location) => ({ ...location })),
    },
    ...(query.name == null ? {} : { name: query.name }),
    entryKinds: query.entryKinds ?? [],
    mimeTypes: query.mimeTypes ?? [],
    ...(query.minSizeBytes == null ? {} : { minSizeBytes: query.minSizeBytes }),
    ...(query.maxSizeBytes == null ? {} : { maxSizeBytes: query.maxSizeBytes }),
    ...(query.modifiedAfter == null ? {} : { modifiedAfter: query.modifiedAfter }),
    ...(query.modifiedBefore == null ? {} : { modifiedBefore: query.modifiedBefore }),
    ...(query.content == null ? {} : { content: query.content }),
    ...(query.semantic == null
      ? {}
      : {
          semantic: {
            query: query.semantic.query,
            libraryId: query.semantic.libraryId,
            scope: query.semantic.scope,
            enrolledRootIds: [...(query.semantic.enrolledRootIds ?? [])],
            ...(query.semantic.selectedConcept == null
              ? {}
              : { selectedConcept: { ...query.semantic.selectedConcept } }),
          },
        }),
    ...(query.concept == null
      ? {}
      : {
          concept: {
            ...query.concept,
            enrolledRootIds: [...(query.concept.enrolledRootIds ?? [])],
          },
        }),
    gitStatuses: query.gitStatuses ?? [],
    tags: query.tags ?? [],
    metadata: { ...(query.metadata ?? {}) },
  };
}

function syncProfileFromDto(profile: SyncProfileDto): SyncProfile {
  return {
    ...profile,
    left: { ...profile.left },
    right: { ...profile.right },
    include: [...profile.include],
    exclude: [...profile.exclude],
    schedule: profile.schedule == null ? null : { ...profile.schedule },
    lastRun:
      profile.lastRun == null
        ? null
        : { ...profile.lastRun, message: profile.lastRun.message ?? null },
  };
}

function filterCriteriaFromDto(criteria: FilterCriteriaDto): FilterCriteria {
  return {
    name: criteria.name,
    kind: criteria.kind,
    minSize: criteria.minSize ?? null,
    maxSize: criteria.maxSize ?? null,
    modifiedWithinDays: criteria.modifiedWithinDays ?? null,
    modifiedOlderThanDays: criteria.modifiedOlderThanDays ?? null,
    hidden: criteria.hidden,
    executable: criteria.executable,
  };
}

export function settingsToDto(settings: Settings): SettingsDto {
  return {
    ...settings,
    defaultColumns: [...settings.defaultColumns],
    defaultStartLocations: [...settings.defaultStartLocations],
    enabledPlugins: [...settings.enabledPlugins],
    multiRenamePresets: settings.multiRenamePresets.map((preset) => ({
      ...preset,
      rules: { ...preset.rules, sequence: { ...preset.rules.sequence } },
    })),
    filterPresets: settings.filterPresets.map((preset) => ({
      ...preset,
      criteria: { ...preset.criteria },
    })),
    syncProfiles: settings.syncProfiles.map((profile) => ({
      ...profile,
      left: { ...profile.left },
      right: { ...profile.right },
      include: [...profile.include],
      exclude: [...profile.exclude],
      schedule: profile.schedule === null ? null : { ...profile.schedule },
      lastRun: profile.lastRun === null ? null : { ...profile.lastRun },
    })),
    savedSearches: settings.savedSearches.map((saved) => ({
      ...saved,
      query: {
        ...saved.query,
        scope: {
          ...saved.query.scope,
          locations: [...saved.query.scope.locations],
        },
        entryKinds: [...saved.query.entryKinds],
        mimeTypes: [...saved.query.mimeTypes],
        gitStatuses: [...saved.query.gitStatuses],
        tags: [...saved.query.tags],
        metadata: { ...saved.query.metadata },
        ...(saved.query.semantic === undefined
          ? {}
          : {
              semantic: {
                ...saved.query.semantic,
                enrolledRootIds: [...saved.query.semantic.enrolledRootIds],
              },
            }),
        ...(saved.query.concept === undefined
          ? {}
          : {
              concept: {
                ...saved.query.concept,
                enrolledRootIds: [...saved.query.concept.enrolledRootIds],
              },
            }),
      },
    })),
    favouriteLocations: settings.favouriteLocations.map((favourite) => ({
      ...favourite,
      location: { ...favourite.location },
    })),
    keybindings: { ...settings.keybindings },
    pluginSettings: { ...settings.pluginSettings },
    recentLocationsByWorkspace: Object.fromEntries(
      Object.entries(settings.recentLocationsByWorkspace).map(([workspaceId, locations]) => [
        workspaceId,
        locations.map((location) => ({ ...location })),
      ]),
    ),
  };
}
