import { describe, expect, it } from 'vitest';

import {
  DEFAULT_ENTRY_FORMAT_SETTINGS,
  type EntryFormatSettings,
  formatEntryModifiedAt,
  formatEntrySize,
} from './entry-formatting';

describe('entry presentation formatting', () => {
  it('uses binary units from the raw byte count', () => {
    expect(formatEntrySize({ kind: 'file', size: 1_536 }, settings('binary'))).toBe('1.5 K');
  });

  it('uses decimal units when selected', () => {
    expect(formatEntrySize({ kind: 'file', size: 1_500 }, settings('decimal'))).toBe('1.5 K');
  });

  it('can display the unscaled raw byte count', () => {
    expect(formatEntrySize({ kind: 'file', size: 1_536 }, settings('bytes'))).toBe('1,536 B');
  });

  it('displays two dashes for a directory with no computed size, and missing file sizes', () => {
    expect(formatEntrySize({ kind: 'directory' }, settings('binary'))).toBe('--');
    expect(formatEntrySize({ kind: 'file' }, settings('binary'))).toBe('--');
  });

  it('formats a directory size once one has been computed (task 0071 Ctrl+.)', () => {
    expect(formatEntrySize({ kind: 'directory', size: 4_096 }, settings('binary'))).toBe('4 K');
  });

  it('formats a raw timestamp with the selected short date format', () => {
    expect(formatEntryModifiedAt('2026-07-31T14:05:00.000Z', dateSettings('short'))).toBe(
      '7/31/26, 2:05 PM',
    );
  });

  it('formats the same raw timestamp with the selected medium date format', () => {
    expect(formatEntryModifiedAt('2026-07-31T14:05:00.000Z', dateSettings('medium'))).toBe(
      'Jul 31, 2026, 2:05 PM',
    );
  });

  it('supports a locale-independent ISO date format and missing timestamps', () => {
    expect(formatEntryModifiedAt('2026-07-31T14:05:00.000Z', dateSettings('iso'))).toBe(
      '2026-07-31 14:05',
    );
    expect(formatEntryModifiedAt(undefined, dateSettings('iso'))).toBe('—');
  });

  it('provides settings defaults outside UI components', () => {
    expect(DEFAULT_ENTRY_FORMAT_SETTINGS).toEqual({
      sizeFormat: 'binary',
      dateFormat: 'medium',
    });
  });
});

function settings(sizeFormat: EntryFormatSettings['sizeFormat']): EntryFormatSettings {
  return {
    sizeFormat,
    dateFormat: 'medium',
    locale: 'en-US',
    timeZone: 'UTC',
  };
}

function dateSettings(dateFormat: EntryFormatSettings['dateFormat']): EntryFormatSettings {
  return {
    sizeFormat: 'binary',
    dateFormat,
    locale: 'en-US',
    timeZone: 'UTC',
  };
}
