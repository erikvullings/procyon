import { describe, expect, it } from 'vitest';
import { type Block, parseInline, parseMarkdown } from './markdown';

describe('parseInline', () => {
  it('reads code, bold and italic', () => {
    expect(parseInline('a `b` **c** *d* _e_ f')).toEqual([
      { kind: 'text', text: 'a ' },
      { kind: 'code', text: 'b' },
      { kind: 'text', text: ' ' },
      { kind: 'bold', text: 'c' },
      { kind: 'text', text: ' ' },
      { kind: 'italic', text: 'd' },
      { kind: 'text', text: ' ' },
      { kind: 'italic', text: 'e' },
      { kind: 'text', text: ' f' },
    ]);
  });

  it('keeps markers that are not closed, and underscores inside words', () => {
    expect(parseInline('2 * 3 and snake_case_name and `open')).toEqual([
      { kind: 'text', text: '2 * 3 and snake_case_name and `open' },
    ]);
  });

  it('does not format inside code', () => {
    expect(parseInline('`**x**`')).toEqual([{ kind: 'code', text: '**x**' }]);
  });
});

describe('parseMarkdown', () => {
  it('reads paragraphs, bullet lists and numbered lists', () => {
    const blocks = parseMarkdown('Intro **here**\n\n- one\n- two `x`\n\n1. first\n2. second');
    expect(blocks).toEqual<Block[]>([
      {
        kind: 'paragraph',
        inline: [
          { kind: 'text', text: 'Intro ' },
          { kind: 'bold', text: 'here' },
        ],
      },
      {
        kind: 'list',
        ordered: false,
        items: [
          [{ kind: 'text', text: 'one' }],
          [
            { kind: 'text', text: 'two ' },
            { kind: 'code', text: 'x' },
          ],
        ],
      },
      {
        kind: 'list',
        ordered: true,
        items: [[{ kind: 'text', text: 'first' }], [{ kind: 'text', text: 'second' }]],
      },
    ]);
  });

  it('starts a list right after a line of text, as models write it', () => {
    const blocks = parseMarkdown('You have:\n- a\n* b');
    expect(blocks.map((block) => block.kind)).toEqual(['paragraph', 'list']);
  });

  it('reads fenced code blocks verbatim', () => {
    expect(parseMarkdown('Run:\n```sh\nls -la\n  echo **x**\n```\nDone')).toEqual<Block[]>([
      { kind: 'paragraph', inline: [{ kind: 'text', text: 'Run:' }] },
      { kind: 'code', text: 'ls -la\n  echo **x**' },
      { kind: 'paragraph', inline: [{ kind: 'text', text: 'Done' }] },
    ]);
  });

  it('closes an unfinished code fence at the end', () => {
    expect(parseMarkdown('```\nls')).toEqual<Block[]>([{ kind: 'code', text: 'ls' }]);
  });

  it('shows headings as bold paragraphs without the # marks', () => {
    expect(parseMarkdown('## Title\ntext')).toEqual<Block[]>([
      { kind: 'heading', inline: [{ kind: 'text', text: 'Title' }] },
      { kind: 'paragraph', inline: [{ kind: 'text', text: 'text' }] },
    ]);
  });

  it('joins wrapped lines of one paragraph and returns nothing for blank text', () => {
    expect(parseMarkdown('a\nb')).toEqual<Block[]>([
      { kind: 'paragraph', inline: [{ kind: 'text', text: 'a b' }] },
    ]);
    expect(parseMarkdown('  \n ')).toEqual([]);
  });
});
