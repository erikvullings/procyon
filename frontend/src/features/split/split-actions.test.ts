import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import {
  customPartBytes,
  isCombinablePart,
  isSplitPartName,
  isSplittable,
  MAX_SPLIT_PARTS,
  partCount,
  SPLIT_PRESETS,
} from './split-actions';

const entry = (name: string, kind: EntrySummary['kind'] = 'file'): EntrySummary =>
  ({
    id: name,
    name,
    kind,
    location: { providerId: 'local', uri: `file:///${name}` },
  }) as EntrySummary;

describe('split actions', () => {
  it('recognises numbered parts but not plain, zero or short suffixes', () => {
    expect(isSplitPartName('movie.mkv.001')).toBe(true);
    expect(isSplitPartName('movie.mkv.1000')).toBe(true);
    expect(isSplitPartName('movie.mkv.000')).toBe(false);
    expect(isSplitPartName('movie.mkv.01')).toBe(false);
    expect(isSplitPartName('movie.mkv')).toBe(false);
    expect(isSplitPartName('.001')).toBe(false);
  });

  it('offers split for files and combine only for part files', () => {
    expect(isSplittable(entry('a.bin'))).toBe(true);
    expect(isSplittable(entry('dir', 'directory'))).toBe(false);
    expect(isCombinablePart(entry('a.bin.001'))).toBe(true);
    expect(isCombinablePart(entry('a.bin'))).toBe(false);
    expect(isCombinablePart(entry('a.bin.001', 'directory'))).toBe(false);
  });

  it('turns a typed size and unit into bytes and rejects invalid input', () => {
    expect(customPartBytes('50', 'MiB')).toBe(50 * 1024 * 1024);
    expect(customPartBytes('1,5', 'KiB')).toBe(1536);
    expect(customPartBytes('0', 'MiB')).toBeUndefined();
    expect(customPartBytes('', 'MiB')).toBeUndefined();
    expect(customPartBytes('-3', 'MiB')).toBeUndefined();
    expect(customPartBytes('abc', 'GiB')).toBeUndefined();
  });

  it('counts parts including the empty file and the exact-multiple edge', () => {
    expect(partCount(0, 10)).toBe(1);
    expect(partCount(10, 10)).toBe(1);
    expect(partCount(11, 10)).toBe(2);
    expect(partCount(100, 1)).toBeLessThanOrEqual(MAX_SPLIT_PARTS);
  });

  it('has a FAT32 preset one byte below 4 GiB', () => {
    expect(SPLIT_PRESETS.find((preset) => preset.id === 'fat32')?.bytes).toBe(4 * 1024 ** 3 - 1);
  });
});
