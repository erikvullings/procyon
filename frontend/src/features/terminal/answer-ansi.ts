/**
 * Turns the assistant's Markdown answer into styled terminal text (task 0336), so a question and
 * its answer can be written into the terminal itself, between the commands, and scroll with them.
 * The model's text is cleaned of control characters first: it can style its answer through
 * Markdown only, never send the terminal an escape sequence.
 */
import { type Block, type Inline, parseInline, parseMarkdown } from './markdown';

const RESET = '\x1b[0m';
const BOLD = ['\x1b[1m', '\x1b[22m'] as const;
const ITALIC = ['\x1b[3m', '\x1b[23m'] as const;
const CODE = ['\x1b[36m', '\x1b[39m'] as const;
const COMMAND = ['\x1b[32m', '\x1b[39m'] as const;
const NEWLINE = '\r\n';
const INDENT = '  ';

/** Removes control characters; tabs become two spaces. Line feeds stay, as `\n`. */
function clean(text: string): string {
  return (
    text
      .replace(/\t/g, '  ')
      // biome-ignore lint/suspicious/noControlCharactersInRegex: removing control bytes is the point
      .replace(/[\u0000-\u0009\u000b-\u001f\u007f-\u009f]/g, '')
  );
}

interface Segment {
  readonly style: Inline['kind'];
  readonly text: string;
}

/** Splits spans into words, so a word keeps the style of each piece it is made of. */
function wordsOf(parts: readonly Inline[]): Segment[][] {
  const words: Segment[][] = [];
  let current: Segment[] = [];
  for (const part of parts) {
    for (const piece of part.text.split(/(\s+)/)) {
      if (piece === '') continue;
      if (/^\s+$/.test(piece)) {
        if (current.length > 0) words.push(current);
        current = [];
      } else {
        current.push({ style: part.kind, text: piece });
      }
    }
  }
  if (current.length > 0) words.push(current);
  return words;
}

function styled(segment: Segment): string {
  switch (segment.style) {
    case 'bold':
      return `${BOLD[0]}${segment.text}${BOLD[1]}`;
    case 'italic':
      return `${ITALIC[0]}${segment.text}${ITALIC[1]}`;
    case 'code':
      return `${CODE[0]}${segment.text}${CODE[1]}`;
    default:
      return segment.text;
  }
}

/** Wraps styled words to `width` columns; the first line starts with `first`, the rest `rest`. */
function wrap(parts: readonly Inline[], width: number, first: string, rest: string): string[] {
  const available = Math.max(10, width - first.length);
  const lines: string[] = [];
  let line = '';
  let length = 0;
  for (const word of wordsOf(parts)) {
    const size = word.reduce((sum, segment) => sum + segment.text.length, 0);
    if (length > 0 && length + 1 + size > available) {
      lines.push(line);
      line = '';
      length = 0;
    }
    line += (length > 0 ? ' ' : '') + word.map(styled).join('');
    length += (length > 0 ? 1 : 0) + size;
  }
  if (length > 0) lines.push(line);
  return lines.map((text, index) => (index === 0 ? first : rest) + text);
}

function plain(parts: readonly Inline[]): string {
  return parts.map((part) => part.text).join('');
}

function renderBlock(block: Block, width: number): string[] {
  switch (block.kind) {
    case 'paragraph':
      return wrap(block.inline, width, INDENT, INDENT);
    case 'heading':
      return [`${INDENT}\x1b[1;4m${plain(block.inline)}\x1b[22;24m`];
    case 'list':
      return block.items.flatMap((item, index) => {
        const marker = block.ordered ? `${index + 1}. ` : '• ';
        return wrap(item, width, INDENT + marker, INDENT + ' '.repeat(marker.length));
      });
    case 'code':
      return block.text
        .split('\n')
        .map((line) => (line === '' ? '' : `${INDENT}${INDENT}${CODE[0]}${line}${CODE[1]}`));
  }
}

const FENCE = /^\s*```/;
const COMMAND_LINE = /^\s*Command:\s*(.+)$/i;

/**
 * An answer as terminal text: paragraphs, lists, code blocks, bold, italic and headings, wrapped
 * to `width`, with blank lines between blocks and a coloured `Command:` line. Empty for blank text.
 */
export function renderAnswerAnsi(text: string, width: number): string {
  const blocks: string[][] = [];
  let buffer: string[] = [];
  const flush = () => {
    for (const block of parseMarkdown(buffer.join('\n'))) blocks.push(renderBlock(block, width));
    buffer = [];
  };
  let inFence = false;
  for (const line of clean(text).split('\n')) {
    if (FENCE.test(line)) inFence = !inFence;
    const command = inFence ? null : COMMAND_LINE.exec(line);
    if (command === null) {
      buffer.push(line);
      continue;
    }
    flush();
    blocks.push([
      `${INDENT}${BOLD[0]}Command:${BOLD[1]} ${COMMAND[0]}${(command[1] ?? '').trim()}${COMMAND[1]}`,
    ]);
  }
  flush();
  return blocks
    .filter((lines) => lines.length > 0)
    .map((lines) => lines.join(NEWLINE) + NEWLINE)
    .join(NEWLINE);
}

const HEADING_LINE = /^\s{0,3}#{1,6}\s+(.*)$/;
const BULLET_LINE = /^\s*[-*+]\s+(.*)$/;
const NUMBERED_LINE = /^\s*(\d+)[.)]\s+(.*)$/;

/** Renders an answer one finished line at a time, as it streams in. */
export interface AnswerStream {
  /** The terminal text for one more line of the answer (without its line break). */
  line(text: string): string;
}

/**
 * Like {@link renderAnswerAnsi}, but for an answer that arrives line by line: each line is
 * rendered on its own, the kind of block it belongs to is tracked, and a blank line is put
 * between different kinds of block. A hard-wrapped paragraph therefore stays on separate lines.
 */
export function createAnswerStream(width: number): AnswerStream {
  type Kind = 'blank' | 'paragraph' | 'list' | 'code' | 'heading' | 'command';
  let kind: Kind | undefined;
  let inFence = false;
  /** A blank line when a different kind of block starts after one that has begun. */
  const start = (next: Exclude<Kind, 'blank'>): string => {
    const gap = kind !== undefined && kind !== 'blank' && kind !== next ? NEWLINE : '';
    kind = next;
    return gap;
  };
  const rows = (lines: readonly string[]) => lines.join(NEWLINE) + NEWLINE;
  return {
    line(raw) {
      const text = clean(raw).replace(/\n/g, ' ');
      if (FENCE.test(text)) {
        inFence = !inFence;
        return inFence ? start('code') : '';
      }
      if (inFence) {
        return text === '' ? NEWLINE : rows([`${INDENT}${INDENT}${CODE[0]}${text}${CODE[1]}`]);
      }
      if (text.trim() === '') {
        if (kind === undefined || kind === 'blank') return '';
        kind = 'blank';
        return NEWLINE;
      }
      const command = COMMAND_LINE.exec(text);
      if (command !== null) {
        return (
          start('command') +
          rows([
            `${INDENT}${BOLD[0]}Command:${BOLD[1]} ${COMMAND[0]}${(command[1] ?? '').trim()}${COMMAND[1]}`,
          ])
        );
      }
      const heading = HEADING_LINE.exec(text);
      if (heading !== null) {
        return (
          start('heading') + rows([`${INDENT}\x1b[1;4m${(heading[1] ?? '').trim()}\x1b[22;24m`])
        );
      }
      const bullet = BULLET_LINE.exec(text);
      const numbered = bullet === null ? NUMBERED_LINE.exec(text) : null;
      if (bullet !== null || numbered !== null) {
        const marker = numbered === null ? '• ' : `${numbered[1]}. `;
        const body = ((bullet ?? numbered)?.[numbered === null ? 1 : 2] ?? '').trim();
        return (
          start('list') +
          rows(wrap(parseInline(body), width, INDENT + marker, INDENT + ' '.repeat(marker.length)))
        );
      }
      return start('paragraph') + rows(wrap(parseInline(text.trim()), width, INDENT, INDENT));
    },
  };
}

/** The person's question as a line in the terminal. */
export function questionAnsi(question: string): string {
  return `\x1b[1;35m❯${RESET} ${BOLD[0]}${clean(question).replace(/\n/g, ' ')}${BOLD[1]}${NEWLINE}`;
}

/** What the assistant looked at, one dimmed line each. */
export function stepsAnsi(steps: readonly string[]): string {
  return steps
    .map((step) => `\x1b[2m${INDENT}↳ ${clean(step).replace(/\n/g, ' ')}\x1b[22m${NEWLINE}`)
    .join('');
}

/** A failure or note in red, on its own line. */
export function noticeAnsi(text: string): string {
  return `\x1b[31m${INDENT}${clean(text).replace(/\n/g, ' ')}\x1b[39m${NEWLINE}`;
}
