/**
 * The chat input under the terminal: `#` typed at an empty shell prompt opens it, so a question
 * is written in a field of its own instead of on the shell's line, where wrapping, line editing
 * and the shell's own echo made it hard to read and easy to lose.
 */

const PASTE_START = '\x1b[200~';
const PASTE_END = '\x1b[201~';

/** What opens the chat input: the text after `#`, and whether it ended with Enter. */
export interface ComposerOpening {
  readonly text: string;
  readonly submit: boolean;
}

/**
 * The opening of the chat input for keyboard input that starts with `#` (typed, or pasted with or
 * without bracketed-paste markers). `undefined` for anything else, so it goes to the shell.
 */
export function composerOpening(data: string): ComposerOpening | undefined {
  let text = data;
  const bracketed = text.startsWith(PASTE_START);
  if (bracketed) {
    text = text.slice(PASTE_START.length);
    if (text.endsWith(PASTE_END)) text = text.slice(0, -PASTE_END.length);
  }
  if (!text.startsWith('#')) return undefined;
  let rest = text.slice(1);
  // Enter typed together with the question sends it; a pasted line break stays in the question.
  const submit = !bracketed && /\r$/.test(rest);
  if (submit) rest = rest.slice(0, -1);
  rest = rest.replace(/\r\n|\r/g, '\n');
  // Editing keys mean the person is working on the shell's line.
  // biome-ignore lint/suspicious/noControlCharactersInRegex: matching control bytes is the point
  if (/[\x00-\x09\x0b-\x1f\x7f]/.test(rest)) return undefined;
  rest = rest.replace(/^ +/, '');
  return { text: rest, submit };
}

/**
 * Whether the screen line under the cursor is a prompt with nothing typed after it, for when the
 * typed input could not be followed (after arrow keys or the shell's own completion).
 */
export function promptIsEmpty(line: string): boolean {
  return /(?:^|[%$>❯»#])$/.test(line.trimEnd());
}

/** The completion helpers read the line as typed in the shell, `#` included. */
export function composerLine(text: string): string {
  return `#${text}`;
}

/** `text` with a completion applied at its end. */
export function completed(
  text: string,
  completion: { readonly erase: number; readonly insert: string },
): string {
  return `${text.slice(0, Math.max(0, text.length - completion.erase))}${completion.insert}`;
}

/** Whether the caret is on the first line, so Up recalls an earlier question. */
export function caretOnFirstLine(text: string, caret: number): boolean {
  return !text.slice(0, caret).includes('\n');
}

/** Whether the caret is on the last line, so Down moves to a newer question. */
export function caretOnLastLine(text: string, caret: number): boolean {
  return !text.slice(caret).includes('\n');
}
