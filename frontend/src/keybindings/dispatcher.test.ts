import { describe, expect, it } from 'vitest';

import type { ActionDescriptor } from '../models';
import {
  detectBindingConflicts,
  dispatchKeybinding,
  footerFunctionKeyBindings,
  getLiveBindings,
  type KeybindingContext,
} from './dispatcher';

const actions: readonly ActionDescriptor[] = [
  {
    id: 'core.copy',
    title: 'Copy',
    category: 'fileOperations',
    defaultShortcuts: [{ key: 'F5' }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.palette',
    title: 'Command palette',
    category: 'navigation',
    defaultShortcuts: [{ key: 'p', ctrl: true }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.newTab',
    title: 'New tab',
    category: 'navigation',
    defaultShortcuts: [{ key: 't', ctrl: true }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.rename',
    title: 'Rename',
    category: 'fileOperations',
    defaultShortcuts: [{ key: 'F2' }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.switchPane',
    title: 'Switch pane',
    category: 'navigation',
    defaultShortcuts: [{ key: 'Tab' }, { key: 'Tab', shift: true }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.nextTab',
    title: 'Next tab',
    category: 'navigation',
    defaultShortcuts: [{ key: 'Tab', ctrl: true }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.newConnection',
    title: 'New connection',
    category: 'navigation',
    defaultShortcuts: [{ key: 'n', ctrl: true }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.swapPanes',
    title: 'Swap panes',
    category: 'navigation',
    defaultShortcuts: [{ key: 'u', ctrl: true }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
];

const table: KeybindingContext = { scope: 'table', platform: 'windows', runtime: 'desktop' };

function defaultCode(key: string): string {
  if (/^[a-zA-Z]$/u.test(key)) return `Key${key.toUpperCase()}`;
  if (/^[0-9]$/u.test(key)) return `Digit${key}`;
  return key;
}

function event(key: string, modifiers: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    key,
    code: defaultCode(key),
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    ...modifiers,
  } as KeyboardEvent;
}

describe('keybinding dispatcher', () => {
  it('uses a user override before the registry default', () => {
    expect(dispatchKeybinding(event('F6'), table, actions, { 'core.copy': 'F6' })).toBe(
      'core.copy',
    );
    expect(dispatchKeybinding(event('F5'), table, actions, { 'core.copy': 'F6' })).toBeUndefined();
  });

  it('resolves the primary modifier once for each platform', () => {
    expect(
      dispatchKeybinding(
        event('p', { metaKey: true }),
        { ...table, platform: 'macos' },
        actions,
        {},
      ),
    ).toBe('core.palette');
    expect(dispatchKeybinding(event('p', { ctrlKey: true }), table, actions, {})).toBe(
      'core.palette',
    );
    expect(
      dispatchKeybinding(
        event('p', { ctrlKey: true }),
        { ...table, platform: 'macos' },
        actions,
        {},
      ),
    ).toBeUndefined();
  });

  it('matches Ctrl+Tab to tab-cycling rather than pane-switching on macOS (Cmd+Tab is OS-reserved)', () => {
    const macos = { ...table, platform: 'macos' as const };
    expect(dispatchKeybinding(event('Tab', { ctrlKey: true }), macos, actions, {})).toBe(
      'core.nextTab',
    );
    expect(dispatchKeybinding(event('Tab'), macos, actions, {})).toBe('core.switchPane');
    expect(dispatchKeybinding(event('Tab', { shiftKey: true }), macos, actions, {})).toBe(
      'core.switchPane',
    );
    expect(dispatchKeybinding(event('Tab', { metaKey: true }), macos, actions, {})).toBeUndefined();
  });

  it('never lets literal Ctrl on macOS fall through to a bare-key binding sharing the same key', () => {
    // core.rename's chord is bare F2 (no modifier). Before this fix, hasPrimaryModifier's
    // macOS check (metaKey && !ctrlKey) returned false for a literal Ctrl+F2 press too, which
    // made the bare F2 binding match anyway - the same defect that made Ctrl+Backspace on
    // macOS silently trigger "parent directory" instead of doing nothing (task 0128 follow-up).
    const macos = { ...table, platform: 'macos' as const };
    expect(dispatchKeybinding(event('F2'), macos, actions, {})).toBe('core.rename');
    expect(dispatchKeybinding(event('F2', { ctrlKey: true }), macos, actions, {})).toBeUndefined();
    expect(dispatchKeybinding(event('F2', { metaKey: true }), macos, actions, {})).toBeUndefined();
  });

  it('matches Alt+<letter> against the physical key (event.code), not event.key (task: Alt+Q quit fix)', () => {
    // macOS composes a dead-key/alternate character into `event.key` when Option is held with a
    // letter (e.g. Option+Q produces 'œ'), so a chord bound to Alt+Q must still match a keydown
    // whose `key` is 'œ' as long as its `code` is 'KeyQ' - matching only ignoring event.key.
    const macos = { ...table, platform: 'macos' as const };
    const quitActions: readonly ActionDescriptor[] = [
      {
        id: 'core.quit',
        title: 'Quit',
        category: 'application',
        defaultShortcuts: [{ key: 'q', alt: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ];
    expect(
      dispatchKeybinding(event('œ', { altKey: true, code: 'KeyQ' }), macos, quitActions, {}),
    ).toBe('core.quit');
  });

  it('does not dispatch table actions while a path input or modal has focus', () => {
    expect(
      dispatchKeybinding(event('F5'), { ...table, scope: 'pathInput' }, actions, {}),
    ).toBeUndefined();
    expect(
      dispatchKeybinding(event('F5'), { ...table, scope: 'modal' }, actions, {}),
    ).toBeUndefined();
  });

  it('detects collisions instead of letting the first action shadow the second', () => {
    expect(
      detectBindingConflicts(actions, { 'core.copy': 'F6', 'core.palette': 'F6' }, table),
    ).toEqual([{ shortcut: 'F6', actionIds: ['core.copy', 'core.palette'] }]);
  });

  it('flags browser-reserved bindings as unavailable while retaining them on desktop', () => {
    expect(
      getLiveBindings(actions, {}, { ...table, runtime: 'browser' }).find(
        (binding) => binding.actionId === 'core.palette',
      )?.available,
    ).toBe(false);
    expect(
      getLiveBindings(actions, {}, table).find((binding) => binding.actionId === 'core.palette')
        ?.available,
    ).toBe(true);
  });

  it('reserves Ctrl+T for the browser tab shortcut too (task 0069 core.newTab)', () => {
    expect(
      getLiveBindings(actions, {}, { ...table, runtime: 'browser' }).find(
        (binding) => binding.actionId === 'core.newTab',
      )?.available,
    ).toBe(false);
    expect(
      getLiveBindings(actions, {}, table).find((binding) => binding.actionId === 'core.newTab')
        ?.available,
    ).toBe(true);
  });

  it('reserves Ctrl+N (new browser window) and Ctrl+U (view source) in browser runtime (task 0128)', () => {
    for (const actionId of ['core.newConnection', 'core.swapPanes']) {
      expect(
        getLiveBindings(actions, {}, { ...table, runtime: 'browser' }).find(
          (binding) => binding.actionId === actionId,
        )?.available,
      ).toBe(false);
      expect(
        getLiveBindings(actions, {}, table).find((binding) => binding.actionId === actionId)
          ?.available,
      ).toBe(true);
    }
  });

  it('always lists footer function keys in ascending F-key order, marking unavailable actions instead of hiding them', () => {
    const bindings = footerFunctionKeyBindings(
      actions,
      {},
      table,
      (action) => action.id === 'core.copy',
    );

    expect(bindings).toEqual([
      {
        actionId: 'core.rename',
        shortcut: 'F2',
        key: 'F2',
        title: 'Rename',
        actionAvailable: false,
      },
      { actionId: 'core.copy', shortcut: 'F5', key: 'F5', title: 'Copy', actionAvailable: true },
    ]);
  });

  it('lists only the function-key bindings matching the held modifiers', () => {
    const modifiedActions: readonly ActionDescriptor[] = [
      {
        id: 'core.view',
        title: 'View',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F3' }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'core.quickLook',
        title: 'Quick Look',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F3', shift: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'core.open',
        title: 'Open',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F3', alt: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'core.edit',
        title: 'Edit',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F4', alt: true, shift: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'core.sortByExtension',
        title: 'Sort by Extension',
        category: 'navigation',
        defaultShortcuts: [{ key: 'F4', ctrl: true }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ];

    expect(
      footerFunctionKeyBindings(modifiedActions, {}, table, () => true, { shift: true }),
    ).toEqual([
      {
        actionId: 'core.quickLook',
        shortcut: 'SHIFT+F3',
        key: 'F3',
        title: 'Quick Look',
        actionAvailable: true,
      },
    ]);
    expect(
      footerFunctionKeyBindings(modifiedActions, {}, table, () => true, { alt: true }).map(
        ({ shortcut, key }) => ({ shortcut, key }),
      ),
    ).toEqual([{ shortcut: 'ALT+F3', key: 'F3' }]);
    expect(
      footerFunctionKeyBindings(modifiedActions, {}, table, () => true, {
        alt: true,
        shift: true,
      }).map(({ shortcut, key }) => ({ shortcut, key })),
    ).toEqual([{ shortcut: 'ALT+SHIFT+F4', key: 'F4' }]);
    expect(
      footerFunctionKeyBindings(modifiedActions, {}, table, () => true, { primary: true }).map(
        ({ shortcut, key }) => ({ shortcut, key }),
      ),
    ).toEqual([{ shortcut: 'CTRL+F4', key: 'F4' }]);
  });

  it('keeps in-app View and Edit visible but omits other permanently unavailable actions', () => {
    const withGatedAction: readonly ActionDescriptor[] = [
      ...actions,
      {
        id: 'core.view',
        title: 'View',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F3' }],
        contextRequirements: { featureAvailable: false },
        source: { kind: 'core' },
      },
      {
        id: 'core.edit',
        title: 'Edit',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F4' }],
        contextRequirements: { featureAvailable: false },
        source: { kind: 'core' },
      },
      {
        id: 'plugin.unavailable',
        title: 'Unavailable',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F6' }],
        contextRequirements: { featureAvailable: false },
        source: { kind: 'plugin', pluginId: 'test' },
      },
    ];

    const bindings = footerFunctionKeyBindings(withGatedAction, {}, table, () => true);

    expect(bindings.find((binding) => binding.actionId === 'core.view')).toBeDefined();
    expect(bindings.find((binding) => binding.actionId === 'core.edit')).toBeDefined();
    expect(bindings.find((binding) => binding.actionId === 'plugin.unavailable')).toBeUndefined();
  });
});
