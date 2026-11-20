/**
 * A small Markdown reader for the assistant's answers (task 0336). It understands what models
 * write in a terminal reply: paragraphs, bullet and numbered lists, fenced code blocks, headings,
 * and inline code, bold and italic. It produces a plain tree that the drawer renders as elements,
 * so nothing from the model is ever inserted as HTML.
 */

/** A span of text inside a paragraph, list item or heading. */
export interface Inline {
  readonly kind: 'text' | 'code' | 'bold' | 'italic';
  readonly text: string;
}

export type Block =
  | { readonly kind: 'paragraph'; readonly inline: readonly Inline[] }
  | { readonly kind: 'heading'; readonly inline: readonly Inline[] }
  | {
      readonly kind: 'list';
      readonly ordered: boolean;
      readonly items: readonly (readonly Inline[])[];
    }
  | { readonly kind: 'code'; readonly text: string };

const isWordChar = (character: string | undefined): boolean =>
  character !== undefined && /[\p{L}\p{N}]/u.test(character);

/** The end of a span opened at `start` by `marker`, or `-1` when it is not closed on this line. */
function closingIndex(text: string, marker: string, from: number): number {
  const end = text.indexOf(marker, from);
  return end > from ? end : -1;
}

/** Splits one line of text into code, bold and italic spans. Unclosed markers stay as text. */
export function parseInline(text: string): Inline[] {
  const parts: Inline[] = [];
  let plain = '';
  const flush = () => {
    if (plain !== '') parts.push({ kind: 'text', text: plain });
    plain = '';
  };
  let index = 0;
  while (index < text.length) {
    const character = text[index] ?? '';
    const next = text[index + 1];
    let end = -1;
    let kind: Inline['kind'] | undefined;
    let marker = '';
    if (character === '`') {
      marker = '`';
      kind = 'code';
    } else if (character === '*' && next === '*') {
      marker = '**';
      kind = 'bold';
    } else if (character === '*' && next !== undefined && next !== ' ') {
      marker = '*';
      kind = 'italic';
    } else if (
      character === '_' &&
      !isWordChar(text[index - 1]) &&
      next !== undefined &&
      next !== ' '
    ) {
      marker = '_';
      kind = 'italic';
    }
    if (kind !== undefined) {
      end = closingIndex(text, marker, index + marker.length);
      if (marker === '_' && end !== -1 && isWordChar(text[end + 1])) end = -1;
    }
    if (kind === undefined || end === -1) {
      plain += character;
      index += 1;
      continue;
    }
    flush();
    parts.push({ kind, text: text.slice(index + marker.length, end) });
    index = end + marker.length;
  }
  flush();
  return parts;
}

const FENCE = /^\s*```/;
const HEADING = /^\s{0,3}#{1,6}\s+(.*)$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;

/** Reads Markdown into blocks. Blank text gives no blocks. */
export function parseMarkdown(text: string): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let list: { ordered: boolean; items: Inline[][] } | undefined;
  const flushParagraph = () => {
    if (paragraph.length > 0) {
      blocks.push({ kind: 'paragraph', inline: parseInline(paragraph.join(' ')) });
    }
    paragraph = [];
  };
  const flushList = () => {
    if (list !== undefined) blocks.push({ kind: 'list', ordered: list.ordered, items: list.items });
    list = undefined;
  };
  const lines = text.split('\n');
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? '';
    if (FENCE.test(line)) {
      flushParagraph();
      flushList();
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !FENCE.test(lines[index] ?? '')) {
        code.push(lines[index] ?? '');
        index += 1;
      }
      blocks.push({ kind: 'code', text: code.join('\n') });
      continue;
    }
    if (line.trim() === '') {
      flushParagraph();
      flushList();
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading !== null) {
      flushParagraph();
      flushList();
      blocks.push({ kind: 'heading', inline: parseInline((heading[1] ?? '').trim()) });
      continue;
    }
    const bullet = BULLET.exec(line);
    const numbered = bullet === null ? NUMBERED.exec(line) : null;
    const item = bullet ?? numbered;
    if (item !== null) {
      flushParagraph();
      const ordered = numbered !== null;
      if (list !== undefined && list.ordered !== ordered) flushList();
      list ??= { ordered, items: [] };
      list.items.push(parseInline((item[1] ?? '').trim()));
      continue;
    }
    flushList();
    paragraph.push(line.trim());
  }
  flushParagraph();
  flushList();
  return blocks;
}
