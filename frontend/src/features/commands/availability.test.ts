import { describe, expect, it } from 'vitest';

import type { ActionDescriptor, EntryId, EntrySummary } from '../../models';
import {
  availableActions,
  type CommandAvailabilityContext,
  evaluateActionAvailability,
  menuActionsForContext,
} from './availability';

function action(
  id: string,
  requirements: ActionDescriptor['contextRequirements'] = {},
): ActionDescriptor {
  return {
    id,
    title: id,
    category: 'test',
    defaultShortcuts: [],
    contextRequirements: requirements,
    source: { kind: 'core' },
  };
}

function entry(kind: EntrySummary['kind'], readOnly = false): EntrySummary {
  return {
    id: `${kind}-${readOnly}` as EntryId,
    location: { providerId: 'file', uri: `mock:///${kind}` },
    name: kind,
    kind,
    hidden: false,
    readOnly,
    metadataRevision: 1,
  };
}

function archiveEntry(name = 'notes.zip'): EntrySummary {
  return {
    id: `archive-${name}` as EntryId,
    location: { providerId: 'local', uri: `file:///${name}` },
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

function context(overrides: Partial<CommandAvailabilityContext> = {}): CommandAvailabilityContext {
  return {
    selectedEntries: [],
    locationWritable: true,
    clipboardHasEntries: false,
    openTerminalSupported: false,
    ...overrides,
  };
}

describe('command availability', () => {
  it('evaluates registry requirements without mutating the action or context', () => {
    const descriptor = action('core.rename', { requiresSingleSelection: true });
    const input = context();

    expect(evaluateActionAvailability(descriptor, input)).toEqual({
      action: descriptor,
      available: false,
      reason: 'Select exactly one item',
    });
    expect(descriptor.contextRequirements).toEqual({ requiresSingleSelection: true });
    expect(input.selectedEntries).toEqual([]);
  });

  it('adapts entry actions for a file, directory, multiple selection, and read-only entries', () => {
    const actions = [
      action('core.open', { requiresSingleSelection: true }),
      action('core.rename', { requiresSingleSelection: true }),
      action('core.copy', { requiresSelection: true }),
      action('core.move', { requiresSelection: true }),
      action('core.delete', { requiresSelection: true }),
    ];

    expect(availableActions(actions, context({ selectedEntries: [entry('file')] }))).toEqual([
      { action: actions[0], available: true },
      { action: actions[1], available: true },
      { action: actions[2], available: true },
      { action: actions[3], available: true },
      { action: actions[4], available: true },
    ]);
    expect(availableActions(actions, context({ selectedEntries: [entry('directory')] }))).toEqual(
      expect.arrayContaining([{ action: actions[0], available: true }]),
    );
    expect(
      availableActions(actions, context({ selectedEntries: [entry('file'), entry('directory')] })),
    ).toEqual(
      expect.arrayContaining([
        { action: actions[0], available: false, reason: 'Select exactly one item' },
        { action: actions[1], available: false, reason: 'Select exactly one item' },
      ]),
    );
    expect(availableActions(actions, context({ selectedEntries: [entry('file', true)] }))).toEqual(
      expect.arrayContaining([
        { action: actions[1], available: false, reason: 'Selected item is read-only' },
        { action: actions[3], available: false, reason: 'Selected item is read-only' },
        { action: actions[4], available: false, reason: 'Selected item is read-only' },
      ]),
    );
  });

  it('composes empty-area location actions from the registry and disables unavailable targets', () => {
    const actions = [
      action('core.createDirectory'),
      action('core.paste'),
      action('core.refresh'),
      action('core.openTerminal'),
      action('core.copy', { requiresSelection: true }),
    ];

    expect(menuActionsForContext(actions, context({ locationWritable: false }))).toEqual([
      { action: actions[0], available: false, reason: 'This location is read-only' },
      { action: actions[1], available: false, reason: 'This location is read-only' },
      { action: actions[2], available: true },
      { action: actions[3], available: false, reason: 'Terminal is not supported by this host' },
    ]);
  });

  it('includes core.revealInSystemFileManager in the selection-context menu (task 0061)', () => {
    const actions = [
      action('core.open', { requiresSingleSelection: true }),
      action('core.revealInSystemFileManager', { requiresSingleSelection: true }),
      action('core.createDirectory'),
    ];

    const menu = menuActionsForContext(actions, context({ selectedEntries: [entry('file')] }));

    expect(menu.map((item) => item.action.id)).toEqual([
      'core.open',
      'core.revealInSystemFileManager',
    ]);
    expect(menu).toEqual([
      { action: actions[0], available: true },
      { action: actions[1], available: true },
    ]);
  });

  it('includes core.view and core.edit in the selection-context menu (tasks 0087/0086)', () => {
    const actions = [
      action('core.open', { requiresSingleSelection: true }),
      action('core.view', { requiresSingleSelection: true }),
      action('core.edit', { requiresSingleSelection: true }),
      action('core.createDirectory'),
    ];

    const menu = menuActionsForContext(actions, context({ selectedEntries: [entry('file')] }));

    expect(menu.map((item) => item.action.id)).toEqual(['core.open', 'core.view', 'core.edit']);
    expect(menu).toEqual([
      { action: actions[0], available: true },
      { action: actions[1], available: true },
      { action: actions[2], available: true },
    ]);
  });

  it('offers Quick Look only for one local file', () => {
    const quickLook = action('core.quickLook', {
      featureAvailable: true,
      requiresSingleSelection: true,
    });
    const localFile = {
      ...entry('file'),
      location: { providerId: 'local', uri: 'file:///tmp/report.pdf' },
    };
    const remoteFile = {
      ...entry('file'),
      location: { providerId: 'sftp', uri: 'sftp://connection/report.pdf' },
    };

    expect(menuActionsForContext([quickLook], context({ selectedEntries: [localFile] }))).toEqual([
      { action: quickLook, available: true },
    ]);
    expect(
      evaluateActionAvailability(quickLook, context({ selectedEntries: [remoteFile] })),
    ).toEqual({
      action: quickLook,
      available: false,
      reason: 'Quick Look is available only for local files',
    });
    expect(
      evaluateActionAvailability(quickLook, context({ selectedEntries: [entry('directory')] })),
    ).toEqual({
      action: quickLook,
      available: false,
      reason: 'Quick Look is available only for local files',
    });
  });

  it('includes the available copy name and path actions in the selection-context menu (task 0093)', () => {
    const actions = [
      action('core.copyRelativePath', { requiresSelection: true }),
      action('core.copyPath', { requiresSelection: true }),
      action('core.rename', { requiresSelection: true }),
      action('core.copyName', { requiresSelection: true }),
    ];

    expect(
      menuActionsForContext(actions, context({ selectedEntries: [entry('file')] })).map(
        (item) => item.action.id,
      ),
    ).toEqual(['core.copyName', 'core.copyPath', 'core.copyRelativePath', 'core.rename']);
  });

  it('includes core.pack and core.moveToArchive for any selection, single or multiple', () => {
    const actions = [
      action('core.pack', { requiresSelection: true }),
      action('core.moveToArchive', { requiresSelection: true }),
    ];

    expect(
      menuActionsForContext(actions, context({ selectedEntries: [entry('file')] })).map(
        (item) => item.action.id,
      ),
    ).toEqual(['core.pack', 'core.moveToArchive']);
    expect(
      menuActionsForContext(
        actions,
        context({ selectedEntries: [entry('file'), entry('directory')] }),
      ).map((item) => item.action.id),
    ).toEqual(['core.pack', 'core.moveToArchive']);
  });

  it('shows core.extract for a single selected archive file, and disables it otherwise', () => {
    const actions = [action('core.extract', { requiresSingleSelection: true })];

    expect(availableActions(actions, context({ selectedEntries: [archiveEntry()] }))).toEqual([
      { action: actions[0], available: true },
    ]);
    expect(availableActions(actions, context({ selectedEntries: [entry('file')] }))).toEqual([
      { action: actions[0], available: false, reason: 'Select an archive file' },
    ]);
    expect(
      availableActions(actions, context({ selectedEntries: [archiveEntry(), entry('file')] })),
    ).toEqual([{ action: actions[0], available: false, reason: 'Select exactly one item' }]);
  });

  it('keeps core.trash available for a read-only selection, unlike core.delete (task 0043)', () => {
    const actions = [
      action('core.trash', { requiresSelection: true }),
      action('core.delete', { requiresSelection: true }),
    ];

    const result = availableActions(actions, context({ selectedEntries: [entry('file', true)] }));

    expect(result).toEqual([
      { action: actions[0], available: true },
      { action: actions[1], available: false, reason: 'Selected item is read-only' },
    ]);
  });

  describe('core.uninstallApplication (task 0148)', () => {
    function appBundleEntry(name = 'Widget.app'): EntrySummary {
      return {
        id: `app-${name}` as EntryId,
        location: { providerId: 'local', uri: `file:///Applications/${name}` },
        name,
        kind: 'file',
        hidden: false,
        readOnly: false,
        metadataRevision: 1,
      };
    }

    it('is available for a sole selected .app-suffixed entry', () => {
      const descriptor = action('core.uninstallApplication', { requiresSingleSelection: true });

      expect(
        evaluateActionAvailability(descriptor, context({ selectedEntries: [appBundleEntry()] })),
      ).toEqual({ action: descriptor, available: true });
    });

    it('is unavailable when the sole selected entry is not a .app bundle', () => {
      const descriptor = action('core.uninstallApplication', { requiresSingleSelection: true });

      expect(
        evaluateActionAvailability(descriptor, context({ selectedEntries: [entry('file')] })),
      ).toEqual({ action: descriptor, available: false, reason: 'Select an application' });
    });

    it('is unavailable with two or more selected entries even if one is a .app bundle', () => {
      const descriptor = action('core.uninstallApplication', { requiresSingleSelection: true });

      expect(
        evaluateActionAvailability(
          descriptor,
          context({ selectedEntries: [appBundleEntry(), entry('file')] }),
        ),
      ).toEqual({ action: descriptor, available: false, reason: 'Select exactly one item' });
    });
  });

  describe('checksum and duplicate commands (task 0077)', () => {
    it('offers checksum calculation for a file selection', () => {
      const descriptor = action('core.calculateChecksum', { requiresSelection: true });
      expect(
        evaluateActionAvailability(
          descriptor,
          context({ selectedEntries: [entry('file')], checksumSupported: true }),
        ).available,
      ).toBe(true);
    });

    it('gates checksum calculation on the provider CHECKSUM capability', () => {
      const descriptor = action('core.calculateChecksum', { requiresSelection: true });
      expect(
        evaluateActionAvailability(
          descriptor,
          context({ selectedEntries: [entry('file')], checksumSupported: false }),
        ),
      ).toEqual({
        action: descriptor,
        available: false,
        reason: 'This location does not support checksums',
      });
    });

    it('rejects a directory-only selection for checksums', () => {
      const descriptor = action('core.calculateChecksum', { requiresSelection: true });
      expect(
        evaluateActionAvailability(
          descriptor,
          context({ selectedEntries: [entry('directory')], checksumSupported: true }),
        ),
      ).toEqual({
        action: descriptor,
        available: false,
        reason: 'Select one or more files',
      });
    });

    it('gates duplicate detection on the CHECKSUM capability and an open directory', () => {
      const descriptor = action('core.findDuplicates');
      expect(
        evaluateActionAvailability(descriptor, context({ checksumSupported: false })).available,
      ).toBe(false);
      expect(
        evaluateActionAvailability(
          descriptor,
          context({ checksumSupported: true, hasActiveLocation: false }),
        ),
      ).toEqual({
        action: descriptor,
        available: false,
        reason: 'Open a directory first',
      });
      expect(
        evaluateActionAvailability(
          descriptor,
          context({ checksumSupported: true, hasActiveLocation: true }),
        ).available,
      ).toBe(true);
    });

    it('needs no selection to find duplicates', () => {
      const descriptor = action('core.findDuplicates');
      expect(
        evaluateActionAvailability(descriptor, context({ selectedEntries: [] })).available,
      ).toBe(true);
    });
  });
});
