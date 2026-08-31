import { describe, expect, it } from 'vitest';

import { archiveFileName } from './archive-create-dialog';

describe('archiveFileName', () => {
  it('adds the selected format extension and rejects unsafe names', () => {
    expect(archiveFileName('backup', 'zip')).toEqual({ value: 'backup.zip' });
    expect(archiveFileName('backup.7z', 'sevenZip')).toEqual({ value: 'backup.7z' });
    expect(archiveFileName('../escape', 'zip')).toEqual({ error: 'Use a single archive name.' });
  });
});
