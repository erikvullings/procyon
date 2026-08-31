import { describe, expect, it } from 'vitest';

import type { EntrySummary, Location } from '../../models';
import { operationForDrop, resolveDropTarget, validateDropTarget } from './drag-drop';

const pane: Location = { providerId: 'file', uri: 'file:///home/user/Documents' };

function entry(kind: EntrySummary['kind'], uri: string): EntrySummary {
  return {
    id: uri,
    location: { providerId: 'file', uri },
    name: uri.split('/').at(-1) ?? uri,
    kind,
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

describe('drag and drop targets', () => {
  it('targets a directory row instead of the containing pane', () => {
    expect(
      resolveDropTarget(pane, entry('directory', 'file:///home/user/Documents/Photos')),
    ).toEqual({ providerId: 'file', uri: 'file:///home/user/Documents/Photos' });
    expect(resolveDropTarget(pane, entry('file', 'file:///home/user/Documents/photo.jpg'))).toEqual(
      pane,
    );
  });

  it('rejects read-only, unavailable, and source-subtree targets before drop', () => {
    const source = { providerId: 'file', uri: 'file:///home/user/Projects' };
    expect(validateDropTarget([source], undefined, true)).toMatchObject({ ok: false });
    expect(validateDropTarget([source], pane, false)).toEqual({
      ok: false,
      message: 'The destination directory is read-only.',
    });
    expect(
      validateDropTarget(
        [source],
        { providerId: 'file', uri: 'file:///home/user/Projects/src' },
        true,
      ),
    ).toEqual({ ok: false, message: 'Cannot drop a location into itself or its subtree.' });
  });

  it('uses move by default and the platform copy modifier without inspecting every source', () => {
    expect(operationForDrop('macos', { altKey: false, ctrlKey: false })).toBe('move');
    expect(operationForDrop('macos', { altKey: true, ctrlKey: false })).toBe('copy');
    expect(operationForDrop('windows', { altKey: false, ctrlKey: true })).toBe('copy');
  });
});
