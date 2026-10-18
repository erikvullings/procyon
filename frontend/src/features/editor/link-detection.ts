/**
 * Finds paths and URLs in text (task 0271). Pure and conservative: it only recognises forms that
 * are clearly paths or http(s) URLs, so ordinary prose such as "and/or" or "1/2" is left alone.
 */

export type DetectedLink =
  | {
      readonly kind: 'url';
      /** The URL, without trailing punctuation. */
      readonly url: string;
      readonly from: number;
      readonly to: number;
    }
  | {
      readonly kind: 'path';
      /** The path as written, without a `:line:col` suffix. */
      readonly path: string;
      /** One-based line from a `path:line` or `path:line:col` suffix. */
      readonly line: number | undefined;
      readonly column: number | undefined;
      readonly from: number;
      readonly to: number;
    };

const URL_PATTERN = /https?:\/\/[^\s<>"'`]+/giu;
const TOKEN_DELIMITERS = /[\s"'`<>()[\]{}|,;]/u;
const TRAILING_PUNCTUATION = /[.,;:!?]+$/u;
const EXTENSION = /\.[A-Za-z0-9]{1,10}$/u;
const WINDOWS_DRIVE = /^[A-Za-z]:[\\/]/u;

/** Drops punctuation that ends a sentence rather than the URL, keeping balanced brackets. */
function trimUrl(url: string): string {
  let result = url;
  for (;;) {
    const trimmed = result.replace(TRAILING_PUNCTUATION, '');
    const last = trimmed.at(-1);
    if (
      last === ')' &&
      (trimmed.match(/\(/gu)?.length ?? 0) < (trimmed.match(/\)/gu)?.length ?? 0)
    ) {
      result = trimmed.slice(0, -1);
    } else if (last === ']' || last === '}') {
      result = trimmed.slice(0, -1);
    } else {
      return trimmed;
    }
  }
}

function safeDecode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/** Splits `path:12:5` into its parts; a drive colon (`C:\`) is not a suffix separator. */
function splitLineSuffix(token: string): {
  readonly path: string;
  readonly line: number | undefined;
  readonly column: number | undefined;
} {
  const driveLength = WINDOWS_DRIVE.test(token) ? 2 : 0;
  const rest = token.slice(driveLength);
  const match = /^(.*?)(?::(\d+))?(?::(\d+))?$/u.exec(rest);
  const path = `${token.slice(0, driveLength)}${match?.[1] ?? rest}`;
  const line = match?.[2] === undefined ? undefined : Number.parseInt(match[2], 10);
  const column = match?.[3] === undefined ? undefined : Number.parseInt(match[3], 10);
  return line === undefined
    ? { path: token, line: undefined, column: undefined }
    : { path, line, column };
}

function looksLikePath(path: string): boolean {
  if (path.length < 2) return false;
  if (path.startsWith('//')) return false;
  if (path === '~' || path.startsWith('~/')) return true;
  if (WINDOWS_DRIVE.test(path)) return true;
  if (path.startsWith('/') || path.startsWith('./') || path.startsWith('../')) {
    return path.length > 1 && path !== '/';
  }
  if (!/[\\/]/u.test(path)) return false;
  const last =
    path
      .split(/[\\/]/u)
      .filter((segment) => segment.length > 0)
      .pop() ?? '';
  return EXTENSION.test(last) && !/^\d+(\.\d+)?$/u.test(last);
}

/** The path or URL covering `offset` (a character index into `line`), if any. */
export function detectLinkInLine(line: string, offset: number): DetectedLink | undefined {
  const covered: [number, number][] = [];
  for (const match of line.matchAll(URL_PATTERN)) {
    const start = match.index ?? 0;
    const url = trimUrl(match[0]);
    const end = start + url.length;
    covered.push([start, start + match[0].length]);
    if (offset >= start && offset <= end) return { kind: 'url', url, from: start, to: end };
  }
  let index = 0;
  while (index < line.length) {
    if (TOKEN_DELIMITERS.test(line[index] ?? ' ')) {
      index += 1;
      continue;
    }
    const start = index;
    while (index < line.length && !TOKEN_DELIMITERS.test(line[index] ?? ' ')) index += 1;
    if (covered.some(([from, to]) => start >= from && start < to)) continue;
    if (offset < start || offset > index) continue;
    const token = line.slice(start, index).replace(TRAILING_PUNCTUATION, '');
    if (token.length === 0) continue;
    const end = start + token.length;
    if (/^file:/iu.test(token)) {
      const { path, line: lineNumber, column } = splitLineSuffix(token);
      const decoded = safeDecode(path.replace(/^file:\/\/(?=\/)/iu, '').replace(/^file:/iu, ''));
      return { kind: 'path', path: decoded, line: lineNumber, column, from: start, to: end };
    }
    const { path, line: lineNumber, column } = splitLineSuffix(token);
    if (!looksLikePath(path)) return undefined;
    return { kind: 'path', path, line: lineNumber, column, from: start, to: end };
  }
  return undefined;
}

/** The path or URL covering the character `position` of `text`, with positions relative to `text`. */
export function detectLinkAt(text: string, position: number): DetectedLink | undefined {
  const clamped = Math.min(Math.max(position, 0), text.length);
  const lineStart = text.lastIndexOf('\n', clamped - 1) + 1;
  const newline = text.indexOf('\n', clamped);
  const lineEnd = newline === -1 ? text.length : newline;
  const hit = detectLinkInLine(text.slice(lineStart, lineEnd), clamped - lineStart);
  return hit === undefined
    ? undefined
    : { ...hit, from: hit.from + lineStart, to: hit.to + lineStart };
}
