import { describe, expect, it } from 'vitest';
import type { MusicDuplicateGroup } from '../../models';
import {
  suggestedRemovals,
  totalReclaimableBytes,
  withoutRemoved,
  wouldRemoveEveryCopy,
} from './music-duplicates-state';

function file(name: string, size: number) {
  return {
    location: { providerId: 'local', uri: `file:///m/${name}` },
    size,
    lossless: false,
  };
}

const group: MusicDuplicateGroup = {
  confidence: 'likely',
  files: [file('a.flac', 30), file('b.mp3', 5), file('c.mp3', 4)],
  keepIndex: 0,
  reclaimableBytes: 9,
};

describe('music duplicate selection helpers', () => {
  it('suggests removing everything but the keeper', () => {
    expect([...suggestedRemovals([group])]).toEqual(['file:///m/b.mp3', 'file:///m/c.mp3']);
  });

  it('blocks ticking the last remaining copy', () => {
    const selected = new Set(['file:///m/b.mp3']);
    expect(wouldRemoveEveryCopy([group], selected, 'file:///m/c.mp3')).toBe(false);
    selected.add('file:///m/a.flac');
    expect(wouldRemoveEveryCopy([group], selected, 'file:///m/c.mp3')).toBe(true);
  });

  it('prunes removed files and dissolves groups that are no longer duplicates', () => {
    const afterOne = withoutRemoved([group], new Set(['file:///m/b.mp3']));
    expect(afterOne[0]?.files.map((f) => f.size)).toEqual([30, 4]);
    expect(totalReclaimableBytes(afterOne)).toBe(4);
    expect(withoutRemoved([group], new Set(['file:///m/b.mp3', 'file:///m/c.mp3']))).toEqual([]);
  });
});
