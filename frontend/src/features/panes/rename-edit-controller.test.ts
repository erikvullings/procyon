import { describe, expect, it } from 'vitest';
import type { EntryId, EntrySummary } from '../../models';
import { createRenameEditingController } from './rename-edit-controller';

function makeEntry(id: string, name: string): EntrySummary {
  return {
    id: id as EntryId,
    name,
    kind: 'file',
    size: 0,
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    location: { providerId: 'file', uri: `file:///test/${name}` },
  };
}

describe('createRenameEditingController', () => {
  it('starts with no active entry', () => {
    const ctrl = createRenameEditingController();
    expect(ctrl.entry).toBeUndefined();
    expect(ctrl.value).toBe('');
    expect(ctrl.error).toBeUndefined();
  });

  it('open sets entry and pre-fills value with the entry name', () => {
    const ctrl = createRenameEditingController();
    const entry = makeEntry('one', 'one.txt');
    ctrl.open(entry);
    expect(ctrl.entry).toBe(entry);
    expect(ctrl.value).toBe('one.txt');
    expect(ctrl.error).toBeUndefined();
  });

  it('updateValue updates the draft and validates it', () => {
    const ctrl = createRenameEditingController();
    ctrl.open(makeEntry('one', 'one.txt'));
    ctrl.updateValue('valid-name.txt');
    expect(ctrl.value).toBe('valid-name.txt');
    expect(ctrl.error).toBeUndefined();
  });

  it('updateValue sets an error for invalid names', () => {
    const ctrl = createRenameEditingController();
    ctrl.open(makeEntry('one', 'one.txt'));
    ctrl.updateValue('../bad/path');
    expect(ctrl.error).toBeDefined();
    expect(ctrl.error).toContain('single');
  });

  it('cancel clears entry and error', () => {
    const ctrl = createRenameEditingController();
    ctrl.open(makeEntry('one', 'one.txt'));
    ctrl.updateValue('../bad');
    ctrl.cancel();
    expect(ctrl.entry).toBeUndefined();
    expect(ctrl.error).toBeUndefined();
  });

  it('commit returns the entry and name when valid, then clears state', () => {
    const ctrl = createRenameEditingController();
    const entry = makeEntry('one', 'one.txt');
    ctrl.open(entry);
    ctrl.updateValue('renamed.txt');
    const result = ctrl.commit();
    expect(result).toEqual({ entry, name: 'renamed.txt' });
    expect(ctrl.entry).toBeUndefined();
  });

  it('commit returns undefined and sets error when value is invalid', () => {
    const ctrl = createRenameEditingController();
    ctrl.open(makeEntry('one', 'one.txt'));
    ctrl.updateValue('../invalid');
    const result = ctrl.commit();
    expect(result).toBeUndefined();
    expect(ctrl.error).toBeDefined();
    expect(ctrl.entry).toBeDefined();
  });

  it('commit with the original entry name (no edit) succeeds', () => {
    const ctrl = createRenameEditingController();
    const entry = makeEntry('one', 'one.txt');
    ctrl.open(entry);
    const result = ctrl.commit();
    expect(result).toEqual({ entry, name: 'one.txt' });
  });
});
