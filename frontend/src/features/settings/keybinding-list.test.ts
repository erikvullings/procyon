import { describe, expect, it } from 'vitest';
import type { ActionDescriptor } from '../../models';
import { keybindingGroups } from './keybinding-list';

function action(
  id: string,
  title: string,
  category: string,
  defaultShortcuts: ActionDescriptor['defaultShortcuts'] = [],
): ActionDescriptor {
  return {
    id,
    title,
    category,
    source: { kind: 'core' },
    defaultShortcuts,
    contextRequirements: {},
  } as unknown as ActionDescriptor;
}

const actions = [
  action('core.paste', 'Paste', 'clipboard', [{ key: 'v', ctrl: true }]),
  action('core.calculateFolderSize', 'Calculate Folder Size', 'fileOperations', [
    { key: '.', ctrl: true },
  ]),
  action('core.copy', 'Copy', 'fileOperations', [{ key: 'F5' }]),
  action('core.parent', 'Parent directory', 'navigation', [{ key: 'Backspace' }]),
  action('plugin.x', 'Zeta', 'sample'),
];

const live = [
  { actionId: 'core.paste', shortcut: 'CTRL+V', available: true },
  { actionId: 'core.calculateFolderSize', shortcut: 'CTRL+.', available: true },
  { actionId: 'core.copy', shortcut: 'F5', available: true },
  { actionId: 'core.parent', shortcut: 'BACKSPACE', available: true },
];

describe('keybindingGroups', () => {
  it('groups by category in palette order and sorts titles alphabetically', () => {
    const groups = keybindingGroups(actions, live, {}, 'macos', '');
    expect(groups.map((group) => group.category)).toEqual([
      'fileOperations',
      'navigation',
      'clipboard',
      'sample',
    ]);
    expect(groups[0]?.rows.map((row) => row.title)).toEqual(['Calculate Folder Size', 'Copy']);
  });

  it('filters by name, by id and by the shortcut as shown on the platform', () => {
    const ids = (query: string, platform: 'macos' | 'windows' = 'macos'): string[] =>
      keybindingGroups(actions, live, {}, platform, query).flatMap((group) =>
        group.rows.map((row) => row.action.id),
      );
    expect(ids('folder')).toEqual(['core.calculateFolderSize']);
    expect(ids('core.par')).toEqual(['core.parent']);
    expect(ids('cmd+.')).toEqual(['core.calculateFolderSize']);
    expect(ids('cmd + v')).toEqual(['core.paste']);
    expect(ids('ctrl+v')).toEqual([]);
    expect(ids('ctrl+v', 'windows')).toEqual(['core.paste']);
    expect(ids('f5')).toEqual(['core.copy']);
    expect(ids('nothing matches this')).toEqual([]);
  });
});
