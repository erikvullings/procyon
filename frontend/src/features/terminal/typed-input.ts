/**
 * What the person has typed on the shell's current input line (task 0333, phase 2). Used for `#`
 * questions and command suggestions. Anything that edits the line in ways this module cannot
 * follow (history recall, cursor movement, completion) makes the text unknown until the next Enter.
 */
export interface TypedInput {
  readonly text: string;
  readonly known: boolean;
  /** Set while the chunks of a bracketed paste are still arriving. */
  readonly pasting?: true;
}

export const EMPTY_INPUT: TypedInput = { text: '', known: true };

/** Applies one chunk of keyboard input to the typed line. `submitted` is set when Enter ran a line. */
export function applyTypedInput(
  state: TypedInput,
  data: string,
): { readonly state: TypedInput; readonly submitted: string | undefined } {
  let text = state.text;
  let known = state.known;
  let pasting = state.pasting === true;
  let submitted: string | undefined;
  // A bracketed paste (`ESC[200~ ... ESC[201~`) is literal text, line breaks included; the shell
  // takes it as typed, so the line stays known.
  const pieces = splitPaste(data);
  for (const piece of pieces) {
    if (piece === PASTE_START) {
      pasting = true;
      continue;
    }
    if (piece === PASTE_END) {
      pasting = false;
      continue;
    }
    if (pasting) {
      text += piece.replace(/\r\n|[\r\n]/g, ' ');
      continue;
    }
    const outcome = applyKeys(text, known, piece);
    text = outcome.text;
    known = outcome.known;
    if (outcome.submitted !== undefined) submitted = outcome.submitted;
  }
  return { state: pasting ? { text, known, pasting: true } : { text, known }, submitted };
}

const PASTE_START = '\x1b[200~';
const PASTE_END = '\x1b[201~';

/** `data` cut into key input and the bracketed-paste markers, in order. */
function splitPaste(data: string): string[] {
  const pieces: string[] = [];
  let rest = data;
  while (rest !== '') {
    const starts = [PASTE_START, PASTE_END]
      .map((marker) => ({ marker, at: rest.indexOf(marker) }))
      .filter((found) => found.at >= 0)
      .sort((a, b) => a.at - b.at);
    const next = starts[0];
    if (next === undefined) {
      pieces.push(rest);
      break;
    }
    if (next.at > 0) pieces.push(rest.slice(0, next.at));
    pieces.push(next.marker);
    rest = rest.slice(next.at + next.marker.length);
  }
  return pieces;
}

function applyKeys(
  start: string,
  wasKnown: boolean,
  data: string,
): { readonly text: string; readonly known: boolean; readonly submitted: string | undefined } {
  let text = start;
  let known = wasKnown;
  let submitted: string | undefined;
  for (const character of data) {
    if (character === '\x1b') return { text: '', known: false, submitted };
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
  return { text, known, submitted };
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
