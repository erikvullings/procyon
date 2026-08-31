import { t } from '../../i18n';
import type { Settings } from '../../models';

/** Deep-clones a settings document so drafts never alias the saved copy. */
export function cloneSettings(settings: Settings): Settings {
  return {
    ...settings,
    defaultColumns: [...settings.defaultColumns],
    keybindings: { ...settings.keybindings },
    enabledPlugins: [...settings.enabledPlugins],
    pluginSettings: { ...settings.pluginSettings },
    defaultStartLocations: [...settings.defaultStartLocations],
    favouriteLocations: settings.favouriteLocations.map((favourite) => ({
      ...favourite,
      location: { ...favourite.location },
    })),
    recentLocationsByWorkspace: Object.fromEntries(
      Object.entries(settings.recentLocationsByWorkspace).map(([workspaceId, locations]) => [
        workspaceId,
        locations.map((location) => ({ ...location })),
      ]),
    ),
    savedSearches: settings.savedSearches.map((saved) => ({
      ...saved,
      query: {
        ...saved.query,
        scope: {
          ...saved.query.scope,
          locations: saved.query.scope.locations.map((location) => ({ ...location })),
        },
        ...(saved.query.name == null ? {} : { name: { ...saved.query.name } }),
        entryKinds: [...(saved.query.entryKinds ?? [])],
        mimeTypes: [...(saved.query.mimeTypes ?? [])],
        ...(saved.query.content == null ? {} : { content: { ...saved.query.content } }),
        gitStatuses: [...(saved.query.gitStatuses ?? [])],
        tags: [...(saved.query.tags ?? [])],
        metadata: { ...(saved.query.metadata ?? {}) },
      },
    })),
  };
}

/** Structural equality for two settings documents (order-sensitive for lists). */
export function isSettingsEqual(a: Settings, b: Settings): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Parses a comma-separated list editor field into a trimmed, non-empty array. */
export function parseListInput(value: string): readonly string[] {
  return value
    .split(',')
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}

/** Formats an array back into the comma-separated text a list editor field displays. */
export function formatListInput(values: readonly string[]): string {
  return values.join(', ');
}

export interface SettingsValidationError {
  readonly field: string;
  readonly message: string;
}

/** Validates the fields the settings editor lets a user type freely. */
export function validateSettingsDraft(draft: Settings): readonly SettingsValidationError[] {
  const errors: SettingsValidationError[] = [];
  if (!Number.isFinite(draft.fontSize) || draft.fontSize < 8 || draft.fontSize > 32) {
    errors.push({ field: 'fontSize', message: t('settings', 'fontSizeRange') });
  }
  if (!Number.isFinite(draft.rowHeight) || draft.rowHeight < 16 || draft.rowHeight > 64) {
    errors.push({ field: 'rowHeight', message: t('settings', 'rowHeightRange') });
  }
  if (!Number.isInteger(draft.operationConcurrency) || draft.operationConcurrency < 1) {
    errors.push({
      field: 'operationConcurrency',
      message: t('settings', 'operationConcurrencyRange'),
    });
  }
  return errors;
}

/** Sets or clears one action's keybinding override, keeping the map otherwise unchanged. */
export function setKeybindingOverride(
  keybindings: Readonly<Record<string, string>>,
  actionId: string,
  shortcutText: string,
): Record<string, string> {
  const next = { ...keybindings };
  const trimmed = shortcutText.trim();
  if (trimmed.length === 0) {
    delete next[actionId];
  } else {
    next[actionId] = trimmed;
  }
  return next;
}
