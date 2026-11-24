import { describe, expect, it } from 'vitest';

import type { EntrySummary, ImageSummary } from '../../models';
import type { EntryFormatSettings } from '../entry-formatting/entry-formatting';
import { captionLines, tooltipRows } from './image-caption';

const SETTINGS: EntryFormatSettings = {
  sizeFormat: 'binary',
  dateFormat: 'iso',
  locale: 'en',
  timeZone: 'UTC',
};

function entry(overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: 'photo.jpg',
    location: { providerId: 'file', uri: 'file:///tmp/photo.jpg' },
    name: 'photo.jpg',
    kind: 'file',
    size: 4_690_000,
    modifiedAt: '2026-08-04T00:00:00.000Z',
    hidden: false,
    readOnly: false,
    extension: 'jpg',
    metadataRevision: 1,
    ...overrides,
  };
}

function summary(overrides: Partial<ImageSummary> = {}): ImageSummary {
  return { location: entry().location, ...overrides };
}

describe('captionLines', () => {
  it('shows file size and dimensions with bit depth, one line per field', () => {
    const lines = captionLines(
      ['size', 'dimensions'],
      entry(),
      summary({ width: 4000, height: 2672, bitDepth: 24 }),
      SETTINGS,
    );

    expect(lines).toEqual(['4.5 M', '4000 × 2672 × 24']);
  });

  it('omits fields the file does not have instead of showing blanks', () => {
    expect(captionLines(['size', 'dimensions', 'camera'], entry(), undefined, SETTINGS)).toEqual([
      '4.5 M',
    ]);
    expect(
      captionLines(['dimensions'], entry(), summary({ width: 640, height: 480 }), SETTINGS),
    ).toEqual(['640 × 480']);
  });

  it('summarises the exposure as aperture, shutter speed and ISO', () => {
    const full = summary({ aperture: 2.8, shutterSeconds: 0.004, iso: 400 });
    expect(captionLines(['exposure'], entry(), full, SETTINGS)).toEqual([
      'f/2.8 · 1/250 s · ISO 400',
    ]);
    expect(captionLines(['exposure'], entry(), summary({ aperture: 8 }), SETTINGS)).toEqual([
      'f/8',
    ]);
    expect(captionLines(['exposure'], entry(), summary(), SETTINGS)).toEqual([]);
  });

  it('writes long exposures in seconds and short ones as a fraction', () => {
    const lines = (shutterSeconds: number): string[] =>
      captionLines(['exposure'], entry(), summary({ shutterSeconds }), SETTINGS);

    expect(lines(2)).toEqual(['2 s']);
    expect(lines(1.5)).toEqual(['1.5 s']);
    expect(lines(1 / 60)).toEqual(['1/60 s']);
    expect(lines(0.0002)).toEqual(['1/5000 s']);
  });

  it('shows the capture time in the configured date format', () => {
    expect(
      captionLines(['dateTaken'], entry(), summary({ takenAt: '2024-03-05T14:07:09' }), SETTINGS),
    ).toEqual(['2024-03-05 14:07']);
  });

  it('shows the rating as stars and skips an unrated image', () => {
    expect(captionLines(['rating'], entry(), summary({ rating: 4 }), SETTINGS)).toEqual(['★★★★☆']);
    expect(captionLines(['rating'], entry(), summary({ rating: 0 }), SETTINGS)).toEqual([]);
  });
});

describe('tooltipRows', () => {
  it('lists the fields present in the file as label/value pairs, in the requested order', () => {
    const rows = tooltipRows(
      ['size', 'dimensions', 'camera', 'aperture', 'shutterSpeed', 'iso', 'focalLength'],
      entry(),
      summary({
        width: 4000,
        height: 2672,
        bitDepth: 24,
        camera: 'ACME Cam 9000',
        aperture: 2.8,
        shutterSeconds: 0.004,
        iso: 400,
        focalLengthMm: 35,
      }),
      SETTINGS,
    );

    expect(rows).toEqual([
      { id: 'size', label: 'Size', value: '4.5 M' },
      { id: 'dimensions', label: 'Dimensions', value: '4000 × 2672 × 24' },
      { id: 'camera', label: 'Camera', value: 'ACME Cam 9000' },
      { id: 'aperture', label: 'Aperture', value: 'f/2.8' },
      { id: 'shutterSpeed', label: 'Shutter speed', value: '1/250 s' },
      { id: 'iso', label: 'ISO', value: '400' },
      { id: 'focalLength', label: 'Focal length', value: '35 mm' },
    ]);
  });

  it('describes flash, white balance and exposure bias in words and omits unknown ones', () => {
    const rows = tooltipRows(
      ['flash', 'whiteBalance', 'exposureBias'],
      entry(),
      summary({ flashFired: false, whiteBalanceAuto: true, exposureBiasEv: -0.7 }),
      SETTINGS,
    );

    expect(rows.map(({ id, value }) => [id, value])).toEqual([
      ['flash', 'No flash'],
      ['whiteBalance', 'Auto'],
      ['exposureBias', '-0.7 EV'],
    ]);
    expect(tooltipRows(['flash', 'whiteBalance'], entry(), summary(), SETTINGS)).toEqual([]);
  });

  it('shows the file modification time even without image metadata', () => {
    expect(tooltipRows(['modified'], entry(), undefined, SETTINGS)).toEqual([
      { id: 'modified', label: 'Modified', value: '2026-08-04 00:00' },
    ]);
  });
});
