/**
 * Pure helpers for terminal assistance (task 0333, phase 1). The drawer gathers the transcript
 * and shows the answer; the functions here decide how much output is sent and which line of an
 * answer is a command the person may insert. Nothing here runs a command.
 */

/** What the person asked for. Mirrors `TerminalAssistKindDto` in the backend. */
export type TerminalAssistKind = 'command' | 'explain' | 'fix';

/** One request to the assistant, as the desktop host receives it. */
export interface TerminalAssistRequest {
  readonly kind: TerminalAssistKind;
  readonly transcript: string;
  readonly request?: string;
  readonly workingDirectory: string;
  readonly shell?: string;
}

/** The assistant's plain-text answer. */
export interface TerminalAssistReply {
  readonly text: string;
}

/** Most terminal lines sent with one request; the newest lines are kept. */
export const MAX_TRANSCRIPT_LINES = 200;

/**
 * The last `maxLines` lines of terminal output, with trailing blank lines dropped so an empty
 * prompt line does not count as output. Returns `''` when nothing but blank lines remain.
 */
export function transcriptTail(lines: readonly string[], maxLines = MAX_TRANSCRIPT_LINES): string {
  let end = lines.length;
  while (end > 0 && (lines[end - 1] ?? '').trim() === '') end -= 1;
  return lines.slice(Math.max(0, end - maxLines), end).join('\n');
}

/**
 * The command in an answer. For a `command` request the answer is the command itself; for `fix`
 * it is the text after the last `Command:` line. Returns `undefined` when there is no command.
 */
export function commandFromAnswer(kind: TerminalAssistKind, text: string): string | undefined {
  if (kind === 'command') {
    return text
      .split('\n')
      .map((line) => line.trim())
      .find((line) => line !== '' && !line.startsWith('```'));
  }
  if (kind !== 'fix') return undefined;
  const matches = [...text.matchAll(/^\s*Command:\s*(.+)$/gim)];
  const last = matches.at(-1)?.[1]?.trim();
  return last === undefined || last === '' ? undefined : last;
}

/** Normalises a command for display and insertion: strips a `$` prompt and surrounding backticks. */
export function cleanCommand(command: string): string {
  return command
    .trim()
    .replace(/^\$\s+/, '')
    .replace(/^`+|`+$/g, '')
    .trim();
}
