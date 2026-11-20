import { describe, expect, it } from 'vitest';
import {
  assistContext,
  cleanCommand,
  commandFromAnswer,
  contextChips,
  EXPLAIN_REQUEST,
  FIX_REQUEST,
  MAX_CONTEXT_BLOCKS,
  MAX_TRANSCRIPT_LINES,
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
