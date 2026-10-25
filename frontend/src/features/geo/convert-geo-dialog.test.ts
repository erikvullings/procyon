import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GeoConversionReport } from '../../models';
import { isConvertibleGeoFile } from './convert-geo-actions';
import {
  ConvertGeoDialog,
  type ConvertGeoDialogAttrs,
  type ConvertGeoRequest,
  canConvert,
} from './convert-geo-dialog';

const request: ConvertGeoRequest = {
  files: [
    { location: { providerId: 'local', uri: 'file:///run.gpx' }, name: 'run.gpx' },
    { location: { providerId: 'local', uri: 'file:///bad.kml' }, name: 'bad.kml' },
  ],
};

const reports: GeoConversionReport[] = [
  {
    location: request.files[0]?.location as never,
    outputName: 'run.kml',
    notes: [
      {
        code: 'timestampsNotInKml',
        message: 'per-point timestamps are not stored in plain KML',
        params: { count: '1' },
      },
    ],
  },
  {
    location: request.files[1]?.location as never,
    notes: [],
    error: {
      code: 'xmlSyntax',
      message: 'invalid XML',
      params: { line: '1', column: '2', detail: 'x' },
    },
  },
];

function attrs(patch: Partial<ConvertGeoDialogAttrs> = {}): ConvertGeoDialogAttrs {
  return {
    request,
    client: { previewGeoConversion: vi.fn().mockResolvedValue(reports) },
    onConvert: vi.fn(),
    onCancel: vi.fn(),
    ...patch,
  };
}

let root: HTMLElement;

function render(dialog: ConvertGeoDialogAttrs): void {
  root ??= document.createElement('div');
  document.body.append(root);
  m.render(root, m(ConvertGeoDialog, dialog));
}

afterEach(() => {
  root?.remove();
  root = undefined as never;
});

describe('canConvert', () => {
  it('needs at least one file that can be converted', () => {
    expect(canConvert(undefined)).toBe(false);
    expect(
      canConvert([
        { location: reports[0]?.location as never, notes: [], error: { code: 'x', message: 'x' } },
      ]),
    ).toBe(false);
    expect(canConvert(reports)).toBe(true);
  });
});

describe('isConvertibleGeoFile', () => {
  it('accepts the readable geographic formats only', () => {
    const entry = (name: string, kind: 'file' | 'directory' = 'file') =>
      ({ name, kind, extension: name.split('.').pop() }) as never;
    expect(isConvertibleGeoFile(entry('a.GPX'))).toBe(true);
    expect(isConvertibleGeoFile(entry('a.kmz'))).toBe(true);
    expect(isConvertibleGeoFile(entry('a.json'))).toBe(false);
    expect(isConvertibleGeoFile(entry('a.gpx', 'directory'))).toBe(false);
  });
});

describe('ConvertGeoDialog', () => {
  it('previews the loss per file and lets the user continue with the chosen format', async () => {
    const dialog = attrs();
    render(dialog);
    render(dialog);
    await vi.waitFor(() => {
      render(dialog);
      expect(root.textContent).toContain('run.gpx becomes run.kml');
    });
    expect(dialog.client.previewGeoConversion).toHaveBeenCalledWith(
      [request.files[0]?.location, request.files[1]?.location],
      'geojson',
      false,
    );
    expect(root.textContent).toContain(
      'Per-point timestamps and heart rates of 1 track(s) are not stored in plain KML.',
    );
    expect(root.textContent).toContain(
      'bad.kml cannot be converted: Invalid XML at line 1, column 2: x',
    );
    const convert = [...root.querySelectorAll('button')].find((b) => b.textContent === 'Convert');
    convert?.click();
    expect(dialog.onConvert).toHaveBeenCalledWith({ targetFormat: 'geojson', csvWkt: false });
  });

  it('checks again when the format changes and offers WKT for CSV', async () => {
    const dialog = attrs();
    render(dialog);
    render(dialog);
    await vi.waitFor(() => expect(dialog.client.previewGeoConversion).toHaveBeenCalledTimes(1));
    const select = root.querySelector('select') as HTMLSelectElement;
    select.value = 'csv';
    select.dispatchEvent(new Event('change'));
    render(dialog);
    await vi.waitFor(() => {
      render(dialog);
      expect(dialog.client.previewGeoConversion).toHaveBeenLastCalledWith(
        expect.anything(),
        'csv',
        false,
      );
    });
    expect(root.textContent).toContain('WKT column');
  });

  it('disables Convert when no file can be converted and reports a failed preview', async () => {
    const failing = attrs({
      client: { previewGeoConversion: vi.fn().mockRejectedValue(new Error('offline')) },
    });
    render(failing);
    render(failing);
    await vi.waitFor(() => {
      render(failing);
      expect(root.textContent).toContain('Could not check the conversion: offline');
    });
    const convert = [...root.querySelectorAll('button')].find((b) => b.textContent === 'Convert');
    expect(convert?.disabled).toBe(true);
  });
});
