import { describe, expect, it } from 'vitest';
import { noticeAnsi, questionAnsi, renderAnswerAnsi, stepsAnsi } from './answer-ansi';

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
