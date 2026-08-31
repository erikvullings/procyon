import { describe, expect, it } from 'vitest';

import type { Settings } from '../../models';
import {
  cloneSettings,
  formatListInput,
  isSettingsEqual,
  parseListInput,
  setKeybindingOverride,
  validateSettingsDraft,
} from './settings-model';

function fixtureSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    schemaVersion: 2,
    theme: 'auto',
    language: 'en',
    fontSize: 13,
    rowHeight: 22,
    dateFormat: 'medium',
    sizeFormat: 'binary',
    showHiddenFiles: false,
    confirmPermanentDelete: true,
    defaultConflictPolicy: 'ask',
    operationConcurrency: 2,
    defaultPaneLayout: 'dual',
    defaultColumns: ['core.name', 'core.size'],
    columnWidths: {},
    keybindings: { 'core.rename': 'F2' },
    enabledPlugins: ['example.plugin'],
    pluginSettings: { 'example.plugin': { flag: true } },
    terminalCommand: null,
    editorCommand: null,
    defaultStartLocations: ['file:///home'],
    favouriteLocations: [],
    recentLocationsByWorkspace: {},
    multiRenamePresets: [],
    savedSearches: [],
    iconTheme: 'generic',
    ...overrides,
  };
}

describe('cloneSettings', () => {
  it('produces a deep copy whose nested collections do not alias the original', () => {
    const original = fixtureSettings();
    const clone = cloneSettings(original);

    (clone.defaultColumns as string[]).push('core.modified');
    (clone.keybindings as Record<string, string>)['core.copy'] = 'F5';
    (clone.defaultStartLocations as string[]).push('file:///tmp');

    expect(original.defaultColumns).toEqual(['core.name', 'core.size']);
    expect(original.keybindings).toEqual({ 'core.rename': 'F2' });
    expect(original.defaultStartLocations).toEqual(['file:///home']);
  });

  it('normalizes search collections omitted by the transport', () => {
    const settings = fixtureSettings({
      savedSearches: [
        {
          id: 'search-1',
          name: 'Documents',
          pinned: true,
          query: {
            schemaVersion: 1,
            scope: {
              locations: [{ providerId: 'local', uri: 'file:///home' }],
              recurse: true,
              showHidden: false,
            },
            entryKinds: undefined,
            mimeTypes: null,
            gitStatuses: undefined,
            tags: null,
            metadata: undefined,
          },
        },
      ],
    } as unknown as Partial<Settings>);

    const clone = cloneSettings(settings);

    expect(clone.savedSearches[0]?.query).toMatchObject({
      entryKinds: [],
      mimeTypes: [],
      gitStatuses: [],
      tags: [],
      metadata: {},
    });
  });
});

describe('isSettingsEqual', () => {
  it('is true for structurally identical documents', () => {
    expect(isSettingsEqual(fixtureSettings(), fixtureSettings())).toBe(true);
  });

  it('is false once any field diverges', () => {
    expect(isSettingsEqual(fixtureSettings(), fixtureSettings({ fontSize: 14 }))).toBe(false);
  });
});

describe('parseListInput / formatListInput', () => {
  it('round-trips a comma-separated list, trimming whitespace and empty entries', () => {
    expect(parseListInput(' core.name ,, core.size ,')).toEqual(['core.name', 'core.size']);
    expect(formatListInput(['core.name', 'core.size'])).toBe('core.name, core.size');
  });
});

describe('validateSettingsDraft', () => {
  it('reports no errors for a valid draft', () => {
    expect(validateSettingsDraft(fixtureSettings())).toEqual([]);
  });

  it('flags an out-of-range font size', () => {
    const errors = validateSettingsDraft(fixtureSettings({ fontSize: 4 }));
    expect(errors).toEqual([{ field: 'fontSize', message: expect.any(String) }]);
  });

  it('flags an out-of-range row height', () => {
    const errors = validateSettingsDraft(fixtureSettings({ rowHeight: 200 }));
    expect(errors.map((error) => error.field)).toEqual(['rowHeight']);
  });

  it('flags a non-integer or zero operation concurrency', () => {
    expect(
      validateSettingsDraft(fixtureSettings({ operationConcurrency: 0 })).map((e) => e.field),
    ).toEqual(['operationConcurrency']);
    expect(
      validateSettingsDraft(fixtureSettings({ operationConcurrency: 1.5 })).map((e) => e.field),
    ).toEqual(['operationConcurrency']);
  });

  it('can report multiple simultaneous errors', () => {
    const errors = validateSettingsDraft(
      fixtureSettings({ fontSize: 4, rowHeight: 4, operationConcurrency: 0 }),
    );
    expect(errors.map((error) => error.field).sort()).toEqual([
      'fontSize',
      'operationConcurrency',
      'rowHeight',
    ]);
  });
});

describe('setKeybindingOverride', () => {
  it('adds a new override without mutating the source map', () => {
    const source = { 'core.rename': 'F2' };
    const next = setKeybindingOverride(source, 'core.copy', 'F5');
    expect(next).toEqual({ 'core.rename': 'F2', 'core.copy': 'F5' });
    expect(source).toEqual({ 'core.rename': 'F2' });
  });

  it('trims the shortcut text before storing it', () => {
    expect(setKeybindingOverride({}, 'core.copy', '  Ctrl+Shift+F5  ')).toEqual({
      'core.copy': 'Ctrl+Shift+F5',
    });
  });

  it('clears an override when the shortcut text is empty', () => {
    expect(setKeybindingOverride({ 'core.rename': 'F2' }, 'core.rename', '   ')).toEqual({});
  });
});
