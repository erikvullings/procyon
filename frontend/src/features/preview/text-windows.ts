/**
 * Where a text window of a large file starts and ends. Windows are read as bytes, so a cut that
 * lands anywhere can split a word, or a multi-byte UTF-8 character. These helpers move each cut to
 * the nearest break instead: a paragraph (a blank line) first, then a line, then a word. The breaks
 * are ASCII bytes, which never occur inside a multi-byte UTF-8 character, so a cut on one is always
 * a character boundary too.
 */

const LINE_FEED = 0x0a;
const SPACE = 0x20;

/** UTF-8 continuation bytes are 10xxxxxx; a character never starts with one. */
function isContinuation(byte: number): boolean {
  return (byte & 0xc0) === 0x80;
}

/**
 * The end of a window that should stop at a break: the last paragraph break in `bytes[floor, end)`,
 * else the last line break, else the last space, returned as the index just after it. Returns
 * `end` unchanged when the window has no break (at least `floor`), so a window never shrinks to
 * nothing, and it never ends inside a character.
 */
export function windowEndAtBreak(bytes: Uint8Array, floor: number, end: number): number {
  const lowest = Math.max(0, Math.min(floor, end));
  for (let index = end - 1; index > lowest; index -= 1) {
    if (bytes[index] === LINE_FEED && bytes[index - 1] === LINE_FEED) return index + 1;
  }
  for (let index = end - 1; index >= lowest; index -= 1) {
    if (bytes[index] === LINE_FEED) return index + 1;
  }
  for (let index = end - 1; index >= lowest; index -= 1) {
    if (bytes[index] === SPACE) return index + 1;
  }
  let cut = end;
  while (cut > lowest && cut < bytes.length && isContinuation(bytes[cut] ?? 0)) cut -= 1;
  return cut;
}

/**
 * The start of a window that should begin at a break: the first paragraph break (else line break,
 * else space) in `bytes[0, before)`, as the index just after it. Returns the first character start
 * when there is no break, so a window never begins inside a character.
 */
export function windowStartAtBreak(bytes: Uint8Array, before: number): number {
  const limit = Math.min(before, bytes.length);
  for (let index = 1; index < limit; index += 1) {
    if (bytes[index] === LINE_FEED && bytes[index - 1] === LINE_FEED) return index + 1;
  }
  for (let index = 0; index < limit; index += 1) {
    if (bytes[index] === LINE_FEED) return index + 1;
  }
  for (let index = 0; index < limit; index += 1) {
    if (bytes[index] === SPACE) return index + 1;
  }
  let start = 0;
  while (start < bytes.length && isContinuation(bytes[start] ?? 0)) start += 1;
  return start;
}
