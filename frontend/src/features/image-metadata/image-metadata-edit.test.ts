import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import {
  buildImageMetadataEdit,
  emptyForm,
  formProblems,
  isMetadataEditable,
  parseCaptureTime,
  supportsRichFields,
} from './image-metadata-edit';

function entry(name: string, kind: EntrySummary['kind'] = 'file'): EntrySummary {
  return { name, kind, extension: name.split('.').pop() } as EntrySummary;
}

describe('format support', () => {
  it('limits editing to formats the backend can rewrite losslessly', () => {
    expect(isMetadataEditable(entry('a.JPG'))).toBe(true);
    expect(isMetadataEditable(entry('a.tiff'))).toBe(true);
    expect(isMetadataEditable(entry('a.webp'))).toBe(true);
    expect(isMetadataEditable(entry('a.png'))).toBe(false);
    expect(isMetadataEditable(entry('a.heic'))).toBe(false);
    expect(isMetadataEditable(entry('photos.jpg', 'directory'))).toBe(false);
  });

  it('allows title, keywords and rating only when every file is a JPEG', () => {
    expect(supportsRichFields([entry('a.jpg'), entry('b.jpeg')])).toBe(true);
    expect(supportsRichFields([entry('a.jpg'), entry('b.webp')])).toBe(false);
    expect(supportsRichFields([])).toBe(false);
  });
});

describe('parseCaptureTime', () => {
  it('reads datetime-local values with or without seconds', () => {
    expect(parseCaptureTime('2024-03-05T14:07')).toEqual({
      year: 2024,
      month: 3,
      day: 5,
      hour: 14,
      minute: 7,
      second: 0,
    });
    expect(parseCaptureTime('2024-03-05T14:07:09')?.second).toBe(9);
  });

  it('rejects impossible values', () => {
    expect(parseCaptureTime('2024-13-05T14:07')).toBeUndefined();
    expect(parseCaptureTime('yesterday')).toBeUndefined();
  });
});

describe('buildImageMetadataEdit', () => {
  it('is undefined while nothing would change', () => {
    expect(buildImageMetadataEdit(emptyForm())).toBeUndefined();
  });

  it('turns filled fields into changes and blank ones into nothing', () => {
    const edit = buildImageMetadataEdit({
      ...emptyForm(),
      description: '  Sunset  ',
      keywords: 'a, b,, c ',
      rating: '4',
      latitude: '52,5',
      longitude: '4.9',
      shiftMinutes: '-60',
      cleared: new Set(['copyright']),
    });
    expect(edit).toEqual({
      backupOriginal: true,
      clearFields: ['copyright'],
      shiftDateSeconds: -3600,
      gps: { latitude: 52.5, longitude: 4.9 },
      description: 'Sunset',
      keywords: ['a', 'b', 'c'],
      rating: 4,
    });
  });

  it('carries the destructive switches and the backup choice', () => {
    expect(
      buildImageMetadataEdit({ ...emptyForm(), removeAll: true, backupOriginal: false }),
    ).toEqual({ backupOriginal: false, removeAll: true });
  });
});

describe('formProblems', () => {
  it('flags invalid dates, shifts and half-filled coordinates', () => {
    expect(formProblems({ ...emptyForm(), dateTaken: 'nope' })).toEqual(['invalidDate']);
    expect(formProblems({ ...emptyForm(), shiftMinutes: '1.5' })).toEqual(['invalidShift']);
    expect(formProblems({ ...emptyForm(), latitude: '10' })).toEqual(['invalidGps']);
    expect(formProblems({ ...emptyForm(), latitude: '95', longitude: '1' })).toEqual([
      'invalidGps',
    ]);
    expect(formProblems({ ...emptyForm(), latitude: '10', longitude: '20' })).toEqual([]);
  });
});
