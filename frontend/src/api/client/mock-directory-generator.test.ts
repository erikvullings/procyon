import { describe, expect, it } from 'vitest';

import { createGeneratedDirectory, GENERATED_DIRECTORY_SIZES } from './mock-directory-generator';

describe('createGeneratedDirectory', () => {
  it('generates the same entries for the same seed without materialising the directory', () => {
    const first = createGeneratedDirectory(1_000_000, 42);
    const second = createGeneratedDirectory(1_000_000, 42);

    expect(first.totalEntries).toBe(1_000_000);
    expect(first.page(500_000, 3)).toEqual(second.page(500_000, 3));
    expect(first.page(500_000, 3)).toHaveLength(3);
  });

  it('changes generated entries when the seed changes', () => {
    const first = createGeneratedDirectory(1_000, 1).page(10, 1);
    const second = createGeneratedDirectory(1_000, 2).page(10, 1);

    expect(first).not.toEqual(second);
  });

  it('supports every required large-directory size', () => {
    expect(GENERATED_DIRECTORY_SIZES).toEqual([1_000, 10_000, 100_000, 1_000_000]);
  });

  it('returns only the available entries at the end of a directory', () => {
    const directory = createGeneratedDirectory(1_000, 7);

    expect(directory.page(998, 100)).toHaveLength(2);
  });
});
