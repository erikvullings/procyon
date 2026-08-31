import type { Location as FileLocation, SearchQuery, Settings } from '../../models';
import type { SettingsDto } from '../generated/models/settingsDto';

export function settingsFromDto(settings: SettingsDto): Settings {
  return {
    ...settings,
    multiRenamePresets: settings.multiRenamePresets.map((preset) => ({
      ...preset,
      rules: { ...preset.rules, sequence: { ...preset.rules.sequence } },
    })),
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
  };
}

function searchQueryFromDto(query: SettingsDto['savedSearches'][number]['query']): SearchQuery {
  return {
    schemaVersion: 1,
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
    gitStatuses: query.gitStatuses ?? [],
    tags: query.tags ?? [],
    metadata: { ...(query.metadata ?? {}) },
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
