import { describe, expect, it } from 'vitest';
import { formatHexDump, HEX_BYTES_PER_ROW } from './hex-preview';

const bytes = (...values: number[]) => new Uint8Array(values);

describe('formatHexDump', () => {
  it('formats offset, hex columns and an ASCII gutter', () => {
    const text = formatHexDump(new TextEncoder().encode('Hello, Procyon!!'), 0);
    expect(text).toBe(
      '00000000  48 65 6c 6c 6f 2c 20 50  72 6f 63 79 6f 6e 21 21  |Hello, Procyon!!|',
    );
  });

  it('pads a short final row so the ASCII gutter stays aligned', () => {
    const [full, short] = formatHexDump(new Uint8Array(17).fill(0x41), 0).split('\n');
    expect(short).toMatch(/^00000010 {2}41 +\|A\|$/);
    expect(short?.indexOf('|')).toBe(full?.indexOf('|'));
  });

  it('renders non-printable bytes as dots', () => {
    expect(formatHexDump(bytes(0x00, 0x1f, 0x7f, 0x80, 0xff, 0x41), 0)).toContain('|.....A|');
  });

  it('offsets rows from the window base, not from zero', () => {
    const lines = formatHexDump(new Uint8Array(HEX_BYTES_PER_ROW * 2), 0x10000).split('\n');
    expect(lines.map((line) => line.slice(0, 8))).toEqual(['00010000', '00010010']);
  });

  it('widens the offset column past 32 bits', () => {
    expect(formatHexDump(bytes(1), 0x1_0000_0000)).toMatch(/^0000000100000000 {2}01/);
  });

  it('returns an empty string for no bytes', () => {
    expect(formatHexDump(new Uint8Array(0), 0)).toBe('');
  });
});
