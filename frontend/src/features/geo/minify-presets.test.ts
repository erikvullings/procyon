import { describe, expect, it } from 'vitest';
import type { GeoMinifyPreview } from '../../models';
import {
  cleanMinify,
  lengthChange,
  percentSmaller,
  presetOf,
  presetOptions,
} from './minify-presets';

describe('presets', () => {
  it('recognises the preset a set of options equals', () => {
    expect(presetOf({})).toBe('whitespace');
    expect(presetOf(presetOptions('web'))).toBe('web');
    expect(presetOf({ precision: 6, removeDuplicates: true })).toBe('share');
    expect(presetOf({ precision: 6 })).toBe('custom');
    expect(presetOf({ dropTime: false, dropElevation: false })).toBe('whitespace');
  });
});

describe('cleanMinify', () => {
  it('drops unset values and clamps the precision', () => {
    expect(cleanMinify({ precision: 99, toleranceMeters: 0, dropTime: false })).toEqual({
      precision: 15,
    });
    expect(cleanMinify({ precision: -2.4, toleranceMeters: 3, dropElevation: true })).toEqual({
      precision: 0,
      toleranceMeters: 3,
      dropElevation: true,
    });
  });
});

describe('size and length summaries', () => {
  it('computes the saving only for a smaller result', () => {
    expect(percentSmaller(1000, 250)).toBe(75);
    expect(percentSmaller(1000, 1000)).toBeUndefined();
    expect(percentSmaller(0, 0)).toBeUndefined();
  });

  it('formats the change of a track length', () => {
    const preview = { distanceBeforeMeters: 1000, distanceAfterMeters: 988 } as GeoMinifyPreview;
    expect(lengthChange(preview)).toBe('-1.2%');
    expect(lengthChange({ ...preview, distanceAfterMeters: 1010 })).toBe('+1.0%');
    expect(lengthChange({ ...preview, distanceBeforeMeters: 0 })).toBeUndefined();
  });
});
