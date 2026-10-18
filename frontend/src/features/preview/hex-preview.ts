/** Bytes rendered per hex-dump row (classic `hexdump -C` layout). */
export const HEX_BYTES_PER_ROW = 16;

const HEX_GROUP_SIZE = 8;

/**
 * Formats `bytes` (a window starting at absolute file offset `baseOffset`) as a `hexdump -C`-style
 * dump: offset, two groups of eight hex bytes, then an ASCII gutter. Pure and DOM-free so the F3
 * viewer's hex mode (the fallback for binary files, task 0142) stays trivially testable.
 */
export function formatHexDump(bytes: Uint8Array, baseOffset: number): string {
  const offsetWidth = baseOffset + bytes.length > 0xffff_ffff ? 16 : 8;
  const rows: string[] = [];
  for (let start = 0; start < bytes.length; start += HEX_BYTES_PER_ROW) {
    const row = bytes.subarray(start, start + HEX_BYTES_PER_ROW);
    let hex = '';
    let ascii = '';
    for (let i = 0; i < HEX_BYTES_PER_ROW; i++) {
      const byte = row[i];
      hex += byte === undefined ? '   ' : `${byte.toString(16).padStart(2, '0')} `;
      if (i === HEX_GROUP_SIZE - 1) hex += ' ';
      if (byte !== undefined)
        ascii += byte >= 0x20 && byte < 0x7f ? String.fromCharCode(byte) : '.';
    }
    const offset = (baseOffset + start).toString(16).padStart(offsetWidth, '0');
    rows.push(`${offset}  ${hex} |${ascii}|`);
  }
  return rows.join('\n');
}
