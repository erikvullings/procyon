import m from 'mithril';
import { describe, expect, it } from 'vitest';

import { ArchiveCreateDialog, archiveFileName } from './archive-create-dialog';

describe('archiveFileName', () => {
  it('adds the selected format extension and rejects unsafe names', () => {
    expect(archiveFileName('backup', 'zip')).toEqual({ value: 'backup.zip' });
    expect(archiveFileName('backup.7z', 'sevenZip')).toEqual({ value: 'backup.7z' });
    expect(archiveFileName('../escape', 'zip')).toEqual({ error: 'Use a single archive name.' });
  });
});

describe('ArchiveCreateDialog', () => {
  it('displays the selected format and ZIP compression level in visible native selects', () => {
    const root = document.createElement('div');
    document.body.appendChild(root);
    m.mount(root, {
      view: () =>
        m(ArchiveCreateDialog, {
          open: true,
          moveSources: false,
          onConfirm: () => undefined,
          onCancel: () => undefined,
        }),
    });

    try {
      const selects = [...root.querySelectorAll<HTMLSelectElement>('select')];
      expect(
        selects.map((select) => [select.className, select.selectedOptions[0]?.textContent]),
      ).toEqual([
        ['browser-default', 'ZIP'],
        ['browser-default', 'Normal'],
      ]);
      const format = selects[0];
      const compression = selects[1];
      if (format === undefined) throw new Error('Archive format select is missing');
      if (compression === undefined) throw new Error('ZIP compression select is missing');
      expect(format.value).toBe('zip');
      expect(compression.value).toBe('6');
      compression.value = '9';
      compression.dispatchEvent(new Event('change', { bubbles: true }));
      m.redraw.sync();
      expect(root.querySelectorAll<HTMLSelectElement>('select')[1]?.value).toBe('9');
      format.value = 'sevenZip';
      format.dispatchEvent(new Event('change', { bubbles: true }));
      m.redraw.sync();
      expect(
        [...root.querySelectorAll<HTMLSelectElement>('select')].map((select) => [
          select.className,
          select.selectedOptions[0]?.textContent,
        ]),
      ).toEqual([['browser-default', '7z']]);
      expect(root.querySelector<HTMLSelectElement>('select')?.value).toBe('sevenZip');
    } finally {
      m.mount(root, null);
      root.remove();
    }
  });
});
