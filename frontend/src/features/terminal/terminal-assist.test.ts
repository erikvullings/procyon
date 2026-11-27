import { describe, expect, it } from 'vitest';
import {
  assistContext,
  cleanCommand,
  commandFromAnswer,
  completeMention,
  completeMentionInTree,
  contextChips,
  EXPLAIN_REQUEST,
  effectiveRemoved,
  FIX_REQUEST,
  LARGE_LISTING_NAMES,
  MAX_CONTEXT_BLOCKS,
  MAX_TRANSCRIPT_LINES,
  mentionText,
  parseMentions,
  type TerminalAssistSource,
  transcriptTail,
} from './terminal-assist';

describe('transcriptTail', () => {
  it('keeps the newest lines and drops trailing blank prompt lines', () => {
    expect(transcriptTail(['a', 'b', 'c', '', '  '])).toBe('a\nb\nc');
  });

  it('caps the transcript at the newest lines', () => {
    const lines = Array.from({ length: MAX_TRANSCRIPT_LINES + 5 }, (_, index) => `line ${index}`);
    const tail = transcriptTail(lines).split('\n');
    expect(tail).toHaveLength(MAX_TRANSCRIPT_LINES);
    expect(tail.at(-1)).toBe(`line ${MAX_TRANSCRIPT_LINES + 4}`);
  });

  it('is empty when only blank lines remain', () => {
    expect(transcriptTail(['', ' '])).toBe('');
  });
});

describe('commandFromAnswer', () => {
  it('takes the first non-fence line of a command answer', () => {
    expect(commandFromAnswer('command', '```sh\nls -la\n```')).toBe('ls -la');
  });

  it('is undefined for a command answer with no command line', () => {
    expect(commandFromAnswer('command', '```\n```')).toBeUndefined();
  });

  it('takes the last Command line of a fix answer', () => {
    const answer = 'The directory is missing.\nCommand: mkdir -p build\nCommand: make build';
    expect(commandFromAnswer('fix', answer)).toBe('make build');
  });

  it('is undefined for a fix answer without a Command line', () => {
    expect(commandFromAnswer('fix', 'Nothing safe to propose.')).toBeUndefined();
  });

  it('never extracts a command from an explanation', () => {
    expect(commandFromAnswer('explain', 'Command: rm -rf /')).toBeUndefined();
  });
});

describe('cleanCommand', () => {
  it('strips a shell prompt and surrounding backticks', () => {
    expect(cleanCommand('$ `git status`')).toBe('git status');
  });
});

const source = (over: Partial<TerminalAssistSource> = {}): TerminalAssistSource => ({
  listing: ['a.txt', 'src/'],
  selected: ['a.txt'],
  blocks: [{ command: 'make', exitCode: 2, output: 'boom' }],
  otherFolder: 'file:///other',
  ...over,
});

describe('parseMentions', () => {
  it('reads a quoted name with spaces', () => {
    expect(parseMentions('summarise @"My File.md" and @b.txt')).toEqual({
      files: ['My File.md', 'b.txt'],
    });
  });

  it('reads single quotes the way a shell writes them', () => {
    expect(parseMentions("Summarize @'ATP 7-100.1  Tactics 2024.pdf' now")).toEqual({
      files: ['ATP 7-100.1  Tactics 2024.pdf'],
    });
  });

  it('finds the files named with @', () => {
    expect(parseMentions('why does @src/lib.rs fail? also @a.txt.')).toEqual({
      files: ['src/lib.rs', 'a.txt'],
    });
  });

  it('ignores emails and a lone @', () => {
    expect(parseMentions('mail me@example.com or @ later')).toEqual({ files: [] });
  });

  it('lists each file once', () => {
    expect(parseMentions('@a.txt @a.txt').files).toEqual(['a.txt']);
  });
});

describe('assistContext', () => {
  it('attaches everything by default and keeps the newest blocks', () => {
    const blocks = Array.from({ length: MAX_CONTEXT_BLOCKS + 2 }, (_, index) => ({
      command: `c${index}`,
      exitCode: 0,
      output: 'x',
    }));
    const context = assistContext(source({ blocks }), new Set());
    expect(context.blocks).toHaveLength(MAX_CONTEXT_BLOCKS);
    expect(context.blocks?.at(-1)?.command).toBe(`c${MAX_CONTEXT_BLOCKS + 1}`);
    expect(context.listing).toEqual(['a.txt', 'src/']);
    expect(context.selected).toEqual(['a.txt']);
    expect(context.otherFolder).toBe('file:///other');
    expect(context.omitEnvironment).toBeUndefined();
  });

  it('leaves out what the person removed', () => {
    const context = assistContext(
      source(),
      new Set(['environment', 'listing', 'selection', 'blocks', 'otherFolder'] as const),
    );
    expect(context).toEqual({ omitEnvironment: true });
  });

  it('adds the files named in the question', () => {
    const context = assistContext(source(), new Set(), parseMentions('see @a.txt'));
    expect(context.files).toEqual(['a.txt']);
  });

  it('trims long block output to its newest lines', () => {
    const output = Array.from({ length: 500 }, (_, index) => `l${index}`).join('\n');
    const context = assistContext(
      source({ blocks: [{ command: 'x', exitCode: 0, output }] }),
      new Set(),
    );
    const lines = context.blocks?.[0]?.output.split('\n') ?? [];
    expect(lines.length).toBeLessThanOrEqual(MAX_TRANSCRIPT_LINES);
    expect(lines.at(-1)).toBe('l499');
  });
});

describe('contextChips', () => {
  it('shows one chip per part that has content, with counts', () => {
    expect(contextChips(source(), new Set(), parseMentions('hi'))).toEqual([
      { id: 'environment', removed: false },
      { id: 'listing', removed: false, count: 2, detail: 'a.txt, src/' },
      { id: 'selection', removed: false, count: 1, detail: 'a.txt' },
      { id: 'otherFolder', removed: false, detail: 'file:///other' },
      { id: 'blocks', removed: false, count: 1, detail: 'make' },
    ]);
  });

  it('keeps a removed chip visible so it can be put back, and lists mentioned files', () => {
    const chips = contextChips(
      source({ selected: [], blocks: [] }),
      new Set(['listing']),
      parseMentions('@a.txt'),
    );
    expect(chips).toContainEqual({ id: 'listing', removed: true, count: 2, detail: 'a.txt, src/' });
    expect(chips).toContainEqual({ id: 'file', removed: false, name: 'a.txt' });
    expect(chips.map((chip) => chip.id)).not.toContain('selection');
    expect(chips.map((chip) => chip.id)).not.toContain('blocks');
  });
});

describe('context chip details', () => {
  it('describes what each chip contains, for its tooltip', () => {
    const chips = contextChips(
      source({ blocks: [{ command: 'make', exitCode: 2, output: 'boom' }] }),
      new Set(),
    );
    const detail = (id: string) => chips.find((chip) => chip.id === id)?.detail;
    expect(detail('listing')).toBe('a.txt, src/');
    expect(detail('selection')).toBe('a.txt');
    expect(detail('otherFolder')).toBe('file:///other');
    expect(detail('blocks')).toBe('make');
    expect(detail('environment')).toBeUndefined();
  });

  it('keeps the block count when the shell did not supply command text', () => {
    const chips = contextChips(
      source({
        blocks: [
          { command: '', exitCode: 0, output: 'output one' },
          { command: '', exitCode: 0, output: 'output two' },
        ],
      }),
      new Set(),
    );
    const blocks = chips.find((chip) => chip.id === 'blocks');
    expect(blocks?.count).toBe(2);
    expect(blocks?.detail).toBeUndefined();
  });

  it('shortens a long listing', () => {
    const listing = Array.from({ length: 40 }, (_, index) => `f${index}`);
    const detail = contextChips(source({ listing }), new Set()).find(
      (chip) => chip.id === 'listing',
    )?.detail;
    expect(detail).toMatch(/^f0, f1,/);
    expect(detail).toMatch(/f29, … \(\+10\)$/);
  });
});

describe('ask requests and answers', () => {
  it('has canned explain and fix questions', () => {
    expect(EXPLAIN_REQUEST).toMatch(/explain/i);
    expect(FIX_REQUEST).toMatch(/fail/i);
  });

  it('takes the last Command: line of an ask answer', () => {
    expect(commandFromAnswer('ask', 'Try this.\nCommand: ls -la')).toBe('ls -la');
    expect(commandFromAnswer('ask', 'No command here.')).toBeUndefined();
  });
});

describe('effectiveRemoved', () => {
  const names = (count: number) => Array.from({ length: count }, (_, index) => `f${index}`);

  it('leaves a large folder listing out until the person switches it on', () => {
    const large = source({ listing: names(LARGE_LISTING_NAMES + 1) });
    expect(effectiveRemoved(large, new Set(), new Set()).has('listing')).toBe(true);
    expect(effectiveRemoved(large, new Set(), new Set(['listing'])).has('listing')).toBe(false);
    expect(effectiveRemoved(large, new Set(['listing']), new Set(['listing'])).has('listing')).toBe(
      true,
    );
  });

  it('keeps a small listing on, and what the person removed removed', () => {
    const small = source({ listing: names(LARGE_LISTING_NAMES) });
    expect(effectiveRemoved(small, new Set(), new Set()).has('listing')).toBe(false);
    expect(effectiveRemoved(small, new Set(['blocks']), new Set()).has('blocks')).toBe(true);
  });
});

describe('completeMention', () => {
  const names = [
    'small_report.md',
    'small_notes.txt',
    'src/',
    'My File.md',
    'My Notes.md',
    'README.md',
  ];

  it('completes the only match and ends the file with a space', () => {
    expect(completeMention('# summarise @READ', names)).toEqual({ erase: 0, insert: 'ME.md ' });
  });

  it('completes a folder without a space so the path can continue', () => {
    expect(completeMention('# look in @sr', names)).toEqual({ erase: 0, insert: 'c/' });
  });

  it('extends to the common prefix of several matches', () => {
    expect(completeMention('# explain @sm', names)).toEqual({ erase: 0, insert: 'all_' });
  });

  it('adds nothing when several matches share no more than is typed', () => {
    expect(completeMention('# explain @small_', names)).toBeUndefined();
  });

  it('adds nothing without an @word at the end, a match, or inside an email', () => {
    expect(completeMention('# explain', names)).toBeUndefined();
    expect(completeMention('# explain @zzz', names)).toBeUndefined();
    expect(completeMention('# mail me@sm', names)).toBeUndefined();
    expect(completeMention('# explain @README.md and', names)).toBeUndefined();
  });

  it('quotes a name with a space, replacing what was typed', () => {
    expect(completeMention('# open @My F', names)).toBeUndefined();
    expect(completeMention('# open @"My F', names)).toEqual({ erase: 0, insert: 'ile.md" ' });
    expect(completeMention('# open @"My', names)).toEqual({ erase: 0, insert: ' ' });
  });

  it('closes the single quote the person opened', () => {
    expect(completeMention("# open @'My F", names)).toEqual({ erase: 0, insert: "ile.md' " });
  });

  it('opens the quote when the only match has a space', () => {
    expect(completeMention('# open @My', ['My File.md', 'a.txt'])).toEqual({
      erase: 2,
      insert: '"My File.md" ',
    });
  });

  it('does not run on into whitespace without a quote', () => {
    expect(completeMention('# open @M', names)).toEqual({ erase: 0, insert: 'y' });
  });
});

describe('mentionText', () => {
  it('quotes only names with whitespace', () => {
    expect(mentionText('a.txt')).toBe('@a.txt');
    expect(mentionText('My File.md')).toBe('@"My File.md"');
  });
});

describe('completeMentionInTree (task 0343)', () => {
  const paths = ['README.md', 'src/', 'src/deep/', 'src/deep/main.rs', 'docs/My Guide.md'];

  it('completes a path prefix like a shell', () => {
    expect(completeMentionInTree('# @src/de', paths)).toEqual({ erase: 0, insert: 'ep/' });
  });

  it('finds a single file by its name anywhere under the root', () => {
    expect(completeMentionInTree('# read @main', paths)).toEqual({
      erase: 5,
      insert: '@src/deep/main.rs ',
    });
    expect(completeMentionInTree('# read @my', paths)).toEqual({
      erase: 3,
      insert: '@"docs/My Guide.md" ',
    });
  });

  it('leaves an ambiguous or unknown name alone', () => {
    expect(completeMentionInTree('# @zzz', paths)).toBeUndefined();
    expect(completeMentionInTree('# @d', ['a/deep/', 'b/docs.md'])).toBeUndefined();
  });
});
