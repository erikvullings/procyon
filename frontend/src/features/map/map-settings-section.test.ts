import m from 'mithril';
import { describe, expect, it, vi } from 'vitest';
import type { MapSettings } from '../../models';
import { validateSettingsDraft } from '../settings/settings-model';
import { DEFAULT_MAP_SETTINGS } from './map-settings';
import { MapSettingsSection, networkNotice } from './map-settings-section';

describe('networkNotice', () => {
  it('says who contacts which host, and when nothing is contacted', () => {
    expect(networkNotice(DEFAULT_MAP_SETTINGS)).toContain('tile.openstreetmap.org');
    expect(networkNotice(DEFAULT_MAP_SETTINGS)).toContain('on your behalf');
    expect(
      networkNotice({
        ...DEFAULT_MAP_SETTINGS,
        basemapKind: 'style',
        styleUrl: 'https://s.example/a.json',
      }),
    ).toContain('directly to s.example');
    expect(networkNotice({ ...DEFAULT_MAP_SETTINGS, onlineTiles: false })).toContain('no map data');
    expect(networkNotice({ ...DEFAULT_MAP_SETTINGS, basemapKind: 'pmtiles' })).toContain(
      'local file',
    );
  });
});

describe('MapSettingsSection', () => {
  function render(settings: MapSettings, onChange: (next: MapSettings) => void): HTMLElement {
    const root = document.createElement('div');
    m.render(root, m(MapSettingsSection, { settings, onChange }));
    return root;
  }

  it('shows the validation problem and resets to the OpenStreetMap default', () => {
    const onChange = vi.fn();
    const invalid: MapSettings = {
      ...DEFAULT_MAP_SETTINGS,
      urlTemplate: 'https://t.example/{z}.png',
    };
    const root = render(invalid, onChange);
    expect(root.querySelector('.fm-field-error')?.textContent).toContain('{z}, {x} and {y}');
    root.querySelector<HTMLButtonElement>('.fm-map-reset')?.click();
    expect(onChange).toHaveBeenCalledWith(DEFAULT_MAP_SETTINGS);
  });

  it('renders only the fields of the selected source', () => {
    const style = render(
      { ...DEFAULT_MAP_SETTINGS, basemapKind: 'style', styleUrl: 'https://s.example/a.json' },
      vi.fn(),
    );
    expect(style.textContent).toContain('Style URL');
    expect(style.textContent).not.toContain('Tile URL template');
    const pmtiles = render({ ...DEFAULT_MAP_SETTINGS, basemapKind: 'pmtiles' }, vi.fn());
    expect(pmtiles.textContent).toContain('.pmtiles');
  });
});

describe('settings validation', () => {
  it('blocks saving an unusable basemap', () => {
    const draft = {
      fontSize: 13,
      rowHeight: 20,
      operationConcurrency: 2,
      defaultColumns: ['core.name'],
      map: { ...DEFAULT_MAP_SETTINGS, urlTemplate: 'ftp://x/{z}/{x}/{y}' },
    } as unknown as Parameters<typeof validateSettingsDraft>[0];
    expect(validateSettingsDraft(draft).map((error) => error.field)).toContain('map');
  });
});
