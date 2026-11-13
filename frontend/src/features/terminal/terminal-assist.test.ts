import { describe, expect, it } from 'vitest';
import {
  cleanCommand,
  commandFromAnswer,
  MAX_TRANSCRIPT_LINES,
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
