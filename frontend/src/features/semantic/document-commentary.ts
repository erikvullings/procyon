import type { EntrySummary } from '../../models';
import { previewExtension } from '../preview/content-preview';

/** Longest line the commentary writes, prefix included. */
export const COMMENT_MAX_COLUMNS = 80;

/** How a file's comments are written: a block, prefixed lines, HTML, or plain text. */
export type CommentStyle =
  | { readonly kind: 'block'; readonly open: string; readonly close: string }
  | { readonly kind: 'line'; readonly prefix: string; readonly hashbang: boolean }
  | { readonly kind: 'html' }
  | { readonly kind: 'plain' };

/** `/* ... *\/` with a ` * ` gutter, as in TypeScript and the other C-style languages. */
const C_BLOCK: CommentStyle = { kind: 'block', open: '/*', close: ' */' };
const HASH: CommentStyle = { kind: 'line', prefix: '# ', hashbang: true };
const DASH: CommentStyle = { kind: 'line', prefix: '-- ', hashbang: false };

const C_STYLE_EXTENSIONS = [
  'c',
  'cc',
  'cjs',
  'cpp',
  'cs',
  'css',
  'cxx',
  'go',
  'h',
  'hpp',
  'java',
  'js',
  'jsx',
  'kt',
  'm',
  'mjs',
  'php',
  'rs',
  'scala',
  'scss',
  'swift',
  'ts',
  'tsx',
];

const COMMENT_STYLES: ReadonlyMap<string, CommentStyle> = new Map<string, CommentStyle>([
  ...C_STYLE_EXTENSIONS.map((extension): [string, CommentStyle] => [extension, C_BLOCK]),
  ...['py', 'pl', 'ps1', 'r', 'rb', 'sh', 'zsh'].map((extension): [string, CommentStyle] => [
    extension,
    HASH,
  ]),
  ...['hs', 'lua', 'sql'].map((extension): [string, CommentStyle] => [extension, DASH]),
  ['md', { kind: 'html' }],
  ['markdown', { kind: 'html' }],
  ['txt', { kind: 'plain' }],
  ['text', { kind: 'plain' }],
  ['log', { kind: 'plain' }],
]);

/** The comment style for `entry`, or `undefined` when its file cannot take a comment. */
export function commentStyleFor(entry: EntrySummary): CommentStyle | undefined {
  const extension = previewExtension(entry);
  return extension === undefined ? undefined : COMMENT_STYLES.get(extension);
}

/** Neutralises whatever would close the comment early. */
function neutralise(text: string, style: CommentStyle): string {
  if (style.kind === 'html') return text.replaceAll('--', '- -');
  if (style.kind === 'block') return text.replaceAll('*/', '* /');
  return text;
}

/** Splits a word longer than `width` into pieces that each fit. */
function splitWord(word: string, width: number): string[] {
  const pieces: string[] = [];
  for (let rest = word; rest.length > width; rest = rest.slice(width)) {
    pieces.push(rest.slice(0, width));
    if (rest.length - width <= width) {
      pieces.push(rest.slice(width));
      return pieces;
    }
  }
  return pieces.length === 0 ? [word] : pieces;
}

/** Word-wraps `text` so no line is longer than `width`; blank lines are kept. */
export function wrapText(text: string, width: number): string[] {
  const lines: string[] = [];
  for (const source of text.split(/\r?\n/)) {
    const words = source
      .trim()
      .split(/\s+/)
      .flatMap((word) => splitWord(word, width));
    if (source.trim() === '') {
      lines.push('');
      continue;
    }
    let current = '';
    for (const word of words) {
      if (current === '') {
        current = word;
      } else if (current.length + 1 + word.length <= width) {
        current += ` ${word}`;
      } else {
        lines.push(current);
        current = word;
      }
    }
    lines.push(current);
  }
  return lines;
}

/** The commentary as lines in the file's comment syntax, none longer than the column limit. */
function commentLines(text: string, style: CommentStyle): string[] {
  const neutral = neutralise(text, style);
  switch (style.kind) {
    case 'block':
      return [
        style.open,
        ...wrapText(neutral, COMMENT_MAX_COLUMNS - ' * '.length).map((line) =>
          line === '' ? ' *' : ` * ${line}`,
        ),
        style.close,
      ];
    case 'line':
      return wrapText(neutral, COMMENT_MAX_COLUMNS - style.prefix.length).map((line) =>
        line === '' ? style.prefix.trimEnd() : style.prefix + line,
      );
    case 'html':
      return ['<!--', ...wrapText(neutral, COMMENT_MAX_COLUMNS), '-->'];
    case 'plain':
      return wrapText(neutral, COMMENT_MAX_COLUMNS);
  }
}

/**
 * Where the commentary goes: after a shebang line, or after a Markdown front-matter block, so
 * the file keeps working; otherwise at the very top.
 */
function insertionIndex(lines: readonly string[], style: CommentStyle): number {
  if (style.kind === 'line' && style.hashbang && lines[0]?.startsWith('#!')) return 1;
  if (style.kind === 'html' && lines[0] === '---') {
    const close = lines.indexOf('---', 1);
    if (close > 0) return close + 1;
  }
  return 0;
}

/** Returns `content` with `text` added as a comment at its top, followed by a blank line. */
export function insertCommentary(content: string, text: string, style: CommentStyle): string {
  const newline = content.includes('\r\n') ? '\r\n' : '\n';
  const lines = content.split(/\r?\n/);
  const index = insertionIndex(lines, style);
  return [...lines.slice(0, index), ...commentLines(text, style), '', ...lines.slice(index)].join(
    newline,
  );
}
