import { describe, expect, it } from 'vitest';
import {
  createAnswerStream,
  noticeAnsi,
  questionAnsi,
  renderAnswerAnsi,
  stepsAnsi,
} from './answer-ansi';

const BOLD = (text: string) => `\x1b[1m${text}\x1b[22m`;

describe('renderAnswerAnsi', () => {
  it('styles bold, italic and code inline', () => {
    expect(renderAnswerAnsi('Hello **big** *very* `x` world', 80)).toBe(
      `  Hello ${BOLD('big')} \x1b[3mvery\x1b[23m \x1b[36mx\x1b[39m world\r\n`,
    );
  });

  it('shows lists with bullets and numbers, and a blank line between blocks', () => {
    expect(renderAnswerAnsi('Intro\n\n- a\n- b\n\n1. c\n2. d', 80)).toBe(
      '  Intro\r\n\r\n  • a\r\n  • b\r\n\r\n  1. c\r\n  2. d\r\n',
    );
  });

  it('indents code blocks and shows headings in bold', () => {
    expect(renderAnswerAnsi('## Title\n```sh\nls -la\n```', 80)).toBe(
      '  \x1b[1;4mTitle\x1b[22;24m\r\n\r\n    \x1b[36mls -la\x1b[39m\r\n',
    );
  });

  it('shows a Command: line on its own, in colour', () => {
    expect(renderAnswerAnsi('Try:\nCommand: ls -la', 80)).toBe(
      `  Try:\r\n\r\n  ${BOLD('Command:')} \x1b[32mls -la\x1b[39m\r\n`,
    );
  });

  it('wraps paragraphs to the width and keeps a hanging indent in lists', () => {
    expect(renderAnswerAnsi('aaa bbb ccc ddd eee fff ggg', 12)).toBe(
      '  aaa bbb\r\n  ccc ddd\r\n  eee fff\r\n  ggg\r\n',
    );
    expect(renderAnswerAnsi('- aaa bbb ccc ddd', 12)).toBe('  • aaa bbb\r\n    ccc ddd\r\n');
  });

  it('never lets the model send terminal control sequences', () => {
    const output = renderAnswerAnsi('a\x1b[2Jb\x07c\r\nd\te', 80);
    expect(output).not.toContain('\x1b[2J');
    expect(output).not.toContain('\x07');
    expect(output.replace(new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*m`, 'g'), '')).toBe(
      '  a[2Jbc d e\r\n',
    );
  });

  it('is empty for blank text', () => {
    expect(renderAnswerAnsi('  \n ', 80)).toBe('');
  });
});

describe('questionAnsi and stepsAnsi', () => {
  it('marks the question and strips control characters from it', () => {
    expect(questionAnsi('why\x1b[2J?')).toBe('\x1b[1;35m❯\x1b[0m \x1b[1mwhy[2J?\x1b[22m\r\n');
  });

  it('shows a notice in red', () => {
    expect(noticeAnsi('The assistant is unavailable.')).toBe(
      '\x1b[31m  The assistant is unavailable.\x1b[39m\r\n',
    );
  });

  it('lists the steps dimmed, or nothing', () => {
    expect(stepsAnsi([])).toBe('');
    expect(stepsAnsi(['Read a.txt'])).toBe('\x1b[2m  ↳ Read a.txt\x1b[22m\r\n');
  });
});

describe('createAnswerStream', () => {
  const feed = (lines: string[], width = 80) => {
    const stream = createAnswerStream(width);
    return lines.map((line) => stream.line(line)).join('');
  };

  it('renders lines one at a time with a blank line between kinds of block', () => {
    expect(feed(['Intro **now**', '- a', '- b', '', 'Next'])).toBe(
      `  Intro ${BOLD('now')}\r\n\r\n  • a\r\n  • b\r\n\r\n  Next\r\n`,
    );
  });

  it('keeps the numbers of a numbered list and wraps with a hanging indent', () => {
    expect(feed(['1. aaa bbb ccc ddd', '2. eee'], 12)).toBe(
      '  1. aaa bbb\r\n     ccc ddd\r\n  2. eee\r\n',
    );
  });

  it('indents code between fences and separates it from the text around it', () => {
    expect(feed(['Run:', '```sh', 'ls -la', '', 'echo hi', '```', 'Done'])).toBe(
      '  Run:\r\n\r\n    \x1b[36mls -la\x1b[39m\r\n\r\n    \x1b[36mecho hi\x1b[39m\r\n\r\n  Done\r\n',
    );
  });

  it('shows headings and a Command: line apart from the text', () => {
    expect(feed(['## Title', 'Try:', 'Command: ls -la'])).toBe(
      `  \x1b[1;4mTitle\x1b[22;24m\r\n\r\n  Try:\r\n\r\n  ${BOLD('Command:')} \x1b[32mls -la\x1b[39m\r\n`,
    );
  });

  it('collapses repeated blank lines and strips control characters', () => {
    expect(feed(['a', '', '', 'b\x1b[2J'])).toBe('  a\r\n\r\n  b[2J\r\n');
  });
});
