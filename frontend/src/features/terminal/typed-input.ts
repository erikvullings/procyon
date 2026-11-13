/**
 * What the person has typed on the shell's current input line (task 0333, phase 2). Used for `#`
 * questions and command suggestions. Anything that edits the line in ways this module cannot
 * follow (history recall, cursor movement, completion) makes the text unknown until the next Enter.
 */
export interface TypedInput {
  readonly text: string;
  readonly known: boolean;
}

export const EMPTY_INPUT: TypedInput = { text: '', known: true };

/** Applies one chunk of keyboard input to the typed line. `submitted` is set when Enter ran a line. */
export function applyTypedInput(
  state: TypedInput,
  data: string,
): { readonly state: TypedInput; readonly submitted: string | undefined } {
  let text = state.text;
  let known = state.known;
  let submitted: string | undefined;
  for (const character of data) {
    if (character === '\x1b') return { state: { text: '', known: false }, submitted };
    if (character === '\r' || character === '\n') {
      submitted = known ? text : undefined;
      text = '';
      known = true;
    } else if (character === '\x7f' || character === '\b') {
      text = [...text].slice(0, -1).join('');
    } else if (character === '\x03' || character === '\x15') {
      // Ctrl+C and Ctrl+U clear the line the shell is editing.
      text = '';
      known = true;
    } else if (character < ' ') {
      // Tab completion and other line edits: the shell's line is no longer known.
      known = false;
    } else {
      text += character;
    }
  }
  return { state: { text, known }, submitted };
}

/**
 * The most recent earlier command that starts with what is typed, as the remainder to accept.
 * Returns `undefined` when nothing matches or the typed text is unknown.
 */
export function suggestionRemainder(
  history: readonly string[],
  typed: TypedInput,
): string | undefined {
  if (!typed.known || typed.text === '' || typed.text.startsWith('#')) return undefined;
  for (let index = history.length - 1; index >= 0; index -= 1) {
    const candidate = history[index] ?? '';
    if (candidate.length > typed.text.length && candidate.startsWith(typed.text)) {
      return candidate.slice(typed.text.length);
    }
  }
  return undefined;
}
