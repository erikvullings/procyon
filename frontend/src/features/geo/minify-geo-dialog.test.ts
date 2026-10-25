import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GeoDocument, GeoMinifyPreview } from '../../models';
import type { MapViewAttrs } from '../map/map-view';
import {
  MinifyGeoDialog,
  type MinifyGeoDialogAttrs,
  type MinifyGeoRequest,
} from './minify-geo-dialog';

const captured: { attrs?: MapViewAttrs } = {};

vi.mock('../map/map-view', () => ({
  MapView: {
    view: ({ attrs }: { attrs: MapViewAttrs }) => {
      captured.attrs = attrs;
      return m('.stub-map');
    },
  },
}));

const request: MinifyGeoRequest = {
  files: [{ location: { providerId: 'local', uri: 'file:///run.gpx' }, name: 'run.gpx' }],
};

const preview: GeoMinifyPreview = {
  originalBytes: 100_000,
  minifiedBytes: 25_000,
  featuresBefore: 1,
  featuresAfter: 1,
  positionsBefore: 1000,
  positionsAfter: 100,
  distanceBeforeMeters: 5000,
  distanceAfterMeters: 4950,
  notes: [
    { code: 'ringsKept', message: '3 area ring(s) were kept unchanged', params: { count: '3' } },
  ],
  collection: { type: 'FeatureCollection', features: [] },
};

const original: GeoDocument = {
  format: 'gpx',
  collection: { type: 'FeatureCollection', features: [] },
  totalFeatures: 1,
  displayedFeatures: 1,
  truncated: false,
  bounds: [4, 52, 5, 53],
  warnings: [],
  skipped: [],
};

function attrs(patch: Partial<MinifyGeoDialogAttrs> = {}): MinifyGeoDialogAttrs {
  return {
    request,
    client: {
      previewGeoMinify: vi.fn().mockResolvedValue(preview),
      readGeoFile: vi.fn().mockResolvedValue(original),
      getSettings: vi.fn().mockRejectedValue(new Error('offline')),
      mapTileUrlTemplate: vi.fn(),
      mediaUrl: vi.fn(),
    },
    onApply: vi.fn(),
    onCancel: vi.fn(),
    ...patch,
  };
}

let root: HTMLElement;

function render(dialog: MinifyGeoDialogAttrs): void {
  if (root === undefined) {
    root = document.createElement('div');
    document.body.append(root);
  }
  m.render(root, m(MinifyGeoDialog, dialog));
}

afterEach(() => {
  vi.useRealTimers();
  root?.remove();
  root = undefined as never;
  delete captured.attrs;
});

async function settled(dialog: MinifyGeoDialogAttrs): Promise<void> {
  render(dialog);
  render(dialog);
  await vi.waitFor(() => {
    render(dialog);
    expect(root.textContent).toContain('smaller');
  });
}

describe('MinifyGeoDialog', () => {
  it('shows the estimate, the effect on points and length, notes and the before/after map', async () => {
    const dialog = attrs();
    await settled(dialog);
    expect(dialog.client.previewGeoMinify).toHaveBeenCalledWith(request.files[0]?.location, {});
    expect(root.textContent).toContain('97.7 KB -> 24.4 KB (75% smaller)');
    expect(root.textContent).toContain('1000 -> 100 points');
    expect(root.textContent).toContain('Length 5.00 km -> 4.95 km (-1.0%)');
    expect(root.textContent).toContain('3 area ring(s) were kept unchanged because');
    await vi.waitFor(() => {
      render(dialog);
      expect(captured.attrs?.comparison).toBe(original.collection);
    });
    expect(captured.attrs?.features).toBe(preview.collection);
    expect(captured.attrs?.dataBounds).toEqual([4, 52, 5, 53]);
  });

  it('re-estimates with the chosen preset and applies it as a new file', async () => {
    const dialog = attrs();
    await settled(dialog);
    const select = root.querySelector('select') as HTMLSelectElement;
    select.value = 'web';
    select.dispatchEvent(new Event('change'));
    await vi.waitFor(() => {
      render(dialog);
      expect(dialog.client.previewGeoMinify).toHaveBeenLastCalledWith(request.files[0]?.location, {
        precision: 5,
        toleranceMeters: 2,
        removeDuplicates: true,
      });
    });
    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Minify')?.click();
    expect(dialog.onApply).toHaveBeenCalledWith({
      minify: { precision: 5, toleranceMeters: 2, removeDuplicates: true },
    });
  });

  it('replacing the original asks for a backup by default and warns when it is switched off', async () => {
    const dialog = attrs();
    await settled(dialog);
    const replace = [...root.querySelectorAll<HTMLInputElement>('input[type="radio"]')][1];
    if (replace === undefined) throw new Error('replace option missing');
    replace.checked = true;
    replace.dispatchEvent(new Event('change'));
    render(dialog);
    const backup = [...root.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')].find(
      (box) => box.parentElement?.textContent?.includes('.orig'),
    );
    expect(backup?.checked).toBe(true);
    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Minify')?.click();
    expect(dialog.onApply).toHaveBeenLastCalledWith({
      minify: {},
      inPlace: true,
      backupOriginal: true,
    });

    if (backup === undefined) throw new Error('backup option missing');
    backup.checked = false;
    backup.dispatchEvent(new Event('change'));
    render(dialog);
    expect(root.textContent).toContain('cannot be undone');
  });

  it('reports an estimate that fails and does not allow applying it', async () => {
    const dialog = attrs({
      client: {
        ...attrs().client,
        previewGeoMinify: vi.fn().mockResolvedValue({
          ...preview,
          error: {
            code: 'xmlSyntax',
            message: 'invalid XML',
            params: { line: '1', column: '1', detail: 'x' },
          },
        }),
      },
    });
    render(dialog);
    render(dialog);
    await vi.waitFor(() => {
      render(dialog);
      expect(root.textContent).toContain('Invalid XML at line 1, column 1: x');
    });
    const apply = [...root.querySelectorAll('button')].find((b) => b.textContent === 'Minify');
    expect(apply?.disabled).toBe(true);
  });
});
