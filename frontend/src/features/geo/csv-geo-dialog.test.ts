import m from 'mithril';
import { describe, expect, it, vi } from 'vitest';
import type { CsvGeoInfo } from '../../models';
import { CsvGeoDialog } from './csv-geo-dialog';

const info: CsvGeoInfo = {
  headers: ['name', 'X', 'Y'],
  delimiter: ',',
  xColumn: 'X',
  yColumn: 'Y',
  crs: 'EPSG:28992',
  crsName: 'Amersfoort / RD New',
  crsGuessed: true,
  columnsDetected: true,
  rows: 2,
  skippedRows: 0,
};

function render(attrs: Partial<Parameters<typeof CsvGeoDialog>[0]> = {}) {
  const root = document.createElement('div');
  document.body.append(root);
  const onApply = vi.fn();
  const onCancel = vi.fn();
  m.render(root, m(CsvGeoDialog, { open: true, info, onApply, onCancel, ...attrs } as never));
  m.render(root, m(CsvGeoDialog, { open: true, info, onApply, onCancel, ...attrs } as never));
  return { root, onApply, onCancel };
}

describe('CsvGeoDialog', () => {
  it('starts from the columns and coordinate system that were used', () => {
    const { root } = render();
    const selects = root.querySelectorAll<HTMLSelectElement>('select');
    expect(selects).toHaveLength(3);
    expect([...(selects[0]?.options ?? [])].map((option) => option.value)).toEqual([
      '',
      'name',
      'X',
      'Y',
    ]);
    expect(root.querySelector<HTMLInputElement>('input[type=text]')?.value).toBe('EPSG:28992');
    expect(root.querySelector('datalist')?.querySelectorAll('option').length).toBeGreaterThan(3);
  });

  it('applies the chosen columns and any EPSG code', () => {
    const { root, onApply } = render();
    const input = root.querySelector<HTMLInputElement>('input[type=text]');
    if (input === null) throw new Error('no crs input');
    input.value = ' EPSG:32631 ';
    input.dispatchEvent(new Event('input'));
    const apply = [...root.querySelectorAll('button')].find((button) =>
      button.textContent?.includes('Show on map'),
    );
    apply?.click();
    expect(onApply).toHaveBeenCalledWith({ xColumn: 'X', yColumn: 'Y', crs: 'EPSG:32631' });
  });
});
