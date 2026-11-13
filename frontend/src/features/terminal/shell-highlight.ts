/**
 * A small, dependency-free highlighter for one line of shell command text (task 0333). It colours
 * the parts a person scans for: the command name, options, quoted strings, variables, operators
 * and comments. It is only for display: it never decides whether a command is valid.
 */

export type ShellTokenKind =
  | 'command'
  | 'option'
  | 'string'
  | 'variable'
  | 'operator'
  | 'comment'
  | 'plain';

export interface ShellToken {
  readonly text: string;
  readonly kind: ShellTokenKind;
}

const OPERATOR_CHARS = '|&;<>()';

/**
 * Splits `line` into tokens whose texts concatenate back to `line`. The first word of each
 * simple command (at the start, or after `|`, `&&`, `;`, ...) is the command; a word that starts
 * with `-` is an option.
 */
export function highlightShell(line: string): ShellToken[] {
  const tokens: ShellToken[] = [];
  let commandExpected = true;
  let index = 0;

  const push = (text: string, kind: ShellTokenKind) => {
    if (text !== '') tokens.push({ text, kind });
  };

  while (index < line.length) {
    const character = line.charAt(index);

    if (/\s/.test(character)) {
      const end = scanWhile(line, index, (c) => /\s/.test(c));
      push(line.slice(index, end), 'plain');
      index = end;
    } else if (character === '#' && (index === 0 || /\s/.test(line.charAt(index - 1)))) {
      push(line.slice(index), 'comment');
      index = line.length;
    } else if (character === '"' || character === "'") {
      const end = quotedEnd(line, index, character);
      push(line.slice(index, end), 'string');
      commandExpected = false;
      index = end;
    } else if (character === '$') {
      const end = variableEnd(line, index);
      push(line.slice(index, end), 'variable');
      commandExpected = false;
      index = end;
    } else if (OPERATOR_CHARS.includes(character)) {
      const end = scanWhile(line, index, (c) => OPERATOR_CHARS.includes(c));
      const text = line.slice(index, end);
      push(text, 'operator');
      // `|`, `&&`, `;` and the like start a new simple command; redirections do not.
      commandExpected = !/[<>]/.test(text);
      index = end;
    } else {
      const end = scanWhile(
        line,
        index,
        (c) => !/\s/.test(c) && !OPERATOR_CHARS.includes(c) && c !== '"' && c !== "'" && c !== '$',
      );
      const word = line.slice(index, end);
      if (word.startsWith('-')) push(word, 'option');
      else if (commandExpected) push(word, 'command');
      else push(word, 'plain');
      if (commandExpected && !word.startsWith('-')) commandExpected = false;
      index = end;
    }
  }
  return tokens;
}

function scanWhile(line: string, start: number, predicate: (character: string) => boolean): number {
  let end = start;
  while (end < line.length && predicate(line.charAt(end))) end += 1;
  return end;
}

/** The index just past a quoted string starting at `start`; an unterminated one runs to the end. */
function quotedEnd(line: string, start: number, quote: string): number {
  for (let index = start + 1; index < line.length; index += 1) {
    const character = line.charAt(index);
    if (quote === '"' && character === '\\') index += 1;
    else if (character === quote) return index + 1;
  }
  return line.length;
}

/** The index just past a `$name` or `${...}` reference starting at `start`. */
function variableEnd(line: string, start: number): number {
  if (line.charAt(start + 1) === '{') {
    const close = line.indexOf('}', start + 2);
    return close === -1 ? line.length : close + 1;
  }
  const end = scanWhile(line, start + 1, (c) => /[A-Za-z0-9_]/.test(c));
  return Math.max(end, start + 1);
}
