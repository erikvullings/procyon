import { describe, expect, it } from 'vitest';

import type { ActionDescriptor } from '../../models';
import { filterPaletteActions } from './command-palette';

const actions: readonly ActionDescriptor[] = [
  {
    id: 'core.createDirectory',
    title: 'New Folder',
    category: 'fileOperations',
    defaultShortcuts: [{ key: 'F7' }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.copyPath',
    title: 'Copy Path',
    category: 'clipboard',
    defaultShortcuts: [],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'plugin.archive.extract',
    title: 'Extract Archive',
    category: 'tools',
    defaultShortcuts: [],
    contextRequirements: { featureAvailable: false },
    source: { kind: 'plugin', pluginId: 'plugin.archive' },
  },
];

const availabilityContext = {
  selectedEntries: [],
  locationWritable: true,
  clipboardHasEntries: false,
  openTerminalSupported: false,
};

describe('command palette filtering', () => {
  it('fuzzy-matches titles, ids, and categories', () => {
    expect(filterPaletteActions(actions, 'nfd', new Map(), availabilityContext)).toMatchObject([
      { action: { id: 'core.createDirectory' }, available: true },
    ]);
    expect(
      filterPaletteActions(actions, 'copy path', new Map(), availabilityContext),
    ).toMatchObject([{ action: { id: 'core.copyPath' } }]);
    expect(filterPaletteActions(actions, 'file op', new Map(), availabilityContext)).toMatchObject([
      { action: { id: 'core.createDirectory' } },
    ]);
  });

  it('ranks stronger fuzzy matches before recently used weaker matches', () => {
    const recent = new Map([['core.copyPath', 200]]);

    expect(
      filterPaletteActions(actions, 'op', recent, availabilityContext).map(
        ({ action }) => action.id,
      ),
    ).toEqual(['core.copyPath', 'core.createDirectory']);
  });

  it('uses recency as a tie-breaker and never makes unavailable actions invocable', () => {
    const results = filterPaletteActions(
      actions,
      '',
      new Map([['core.copyPath', 100]]),
      availabilityContext,
    );

    expect(results.map(({ action }) => action.id)).toEqual([
      'core.copyPath',
      'core.createDirectory',
      'plugin.archive.extract',
    ]);
    expect(results.at(-1)).toMatchObject({
      available: false,
      unavailableReason: 'Not available yet',
    });
  });

  it('filters several hundred registry actions without dropping results', () => {
    const manyActions = Array.from(
      { length: 400 },
      (_, index): ActionDescriptor => ({
        id: `plugin.example.command${index}`,
        title: `Example command ${index}`,
        category: 'tools',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'plugin', pluginId: 'plugin.example' },
      }),
    );

    expect(filterPaletteActions(manyActions, 'cmd', new Map(), availabilityContext)).toHaveLength(
      400,
    );
  });
});
