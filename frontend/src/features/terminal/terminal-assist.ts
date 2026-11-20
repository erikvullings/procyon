/**
 * Pure helpers for terminal assistance (task 0333, phase 1). The drawer gathers the transcript
 * and shows the answer; the functions here decide how much output is sent and which line of an
 * answer is a command the person may insert. Nothing here runs a command.
 */

/** What the person asked for. Mirrors `TerminalAssistKindDto` in the backend. */
export type TerminalAssistKind = 'command' | 'explain' | 'fix' | 'ask';

/** One finished command, with its output as shown on screen. */
export interface TerminalBlockContext {
  readonly command: string;
  readonly exitCode: number;
  readonly output: string;
}

/** What the person lets the assistant see besides the question (task 0336). */
export interface TerminalAssistContext {
  readonly omitEnvironment?: boolean;
  readonly listing?: readonly string[];
  readonly selected?: readonly string[];
  readonly otherFolder?: string;
  readonly blocks?: readonly TerminalBlockContext[];
  readonly files?: readonly string[];
}

/** The outcome of a command the assistant asked to run. */
export interface TerminalToolResult {
  readonly command: string;
  readonly declined?: boolean;
  readonly exitCode?: number;
  readonly output?: string;
}

/** One request to the assistant, as the desktop host receives it. */
export interface TerminalAssistRequest {
  readonly kind: TerminalAssistKind;
  readonly transcript: string;
  readonly request?: string;
  readonly workingDirectory: string;
  readonly shell?: string;
  readonly sessionId?: string;
  readonly context?: TerminalAssistContext;
  readonly toolResult?: TerminalToolResult;
}

/** The assistant's answer: text, what it looked at, and a command it asks to run. */
export interface TerminalAssistReply {
  readonly text: string;
  readonly steps?: readonly string[];
  readonly runCommand?: string;
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
 * and `ask` it is the text after the last `Command:` line. Returns `undefined` when there is none.
 */
export function commandFromAnswer(kind: TerminalAssistKind, text: string): string | undefined {
  if (kind === 'command') {
    return text
      .split('\n')
      .map((line) => line.trim())
      .find((line) => line !== '' && !line.startsWith('```'));
  }
  if (kind !== 'fix' && kind !== 'ask') return undefined;
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

/** The question an Explain action asks in a session. */
export const EXPLAIN_REQUEST =
  'Explain the output of my last command: what happened, and whether it is an error.';
/** The question a Fix action asks in a session. */
export const FIX_REQUEST =
  'My last command failed. Explain the most likely cause, then suggest one corrected command.';

/** Most recent command blocks sent with a question. */
export const MAX_CONTEXT_BLOCKS = 5;

/** The parts of a request the person can remove with a chip. */
export type RemovableChipId = 'environment' | 'listing' | 'selection' | 'otherFolder' | 'blocks';

/** What the drawer knows about the terminal and the file manager when the person asks. */
export interface TerminalAssistSource {
  readonly listing: readonly string[];
  readonly selected: readonly string[];
  readonly blocks: readonly TerminalBlockContext[];
  readonly otherFolder?: string | undefined;
}

/** The files named with `@path` in a question. */
export interface Mentions {
  readonly files: readonly string[];
}

/** Finds the `@path` mentions in a question. An `@` inside a word (an email) is not a mention. */
export function parseMentions(text: string): Mentions {
  const files: string[] = [];
  for (const match of text.matchAll(/(?:^|\s)@(\S+)/g)) {
    const name = (match[1] ?? '').replace(/[,;:!?)\]]+$/, '').replace(/\.+$/, '');
    if (name !== '' && !files.includes(name)) files.push(name);
  }
  return { files };
}

const NO_MENTIONS: Mentions = { files: [] };

/**
 * What goes to the assistant: everything by default, minus the parts the person removed, and the
 * `@path` files named in the question.
 */
export function assistContext(
  source: TerminalAssistSource,
  removed: ReadonlySet<RemovableChipId>,
  mentions: Mentions = NO_MENTIONS,
): TerminalAssistContext {
  const keep = (id: RemovableChipId) => !removed.has(id);
  const blocks = keep('blocks')
    ? source.blocks.slice(-MAX_CONTEXT_BLOCKS).map((block) => ({
        ...block,
        output: transcriptTail(block.output.split('\n')),
      }))
    : [];
  const listing = keep('listing') ? source.listing : [];
  const selected = keep('selection') ? source.selected : [];
  return {
    ...(removed.has('environment') ? { omitEnvironment: true } : {}),
    ...(listing.length > 0 ? { listing } : {}),
    ...(selected.length > 0 ? { selected } : {}),
    ...(keep('otherFolder') && source.otherFolder ? { otherFolder: source.otherFolder } : {}),
    ...(blocks.length > 0 ? { blocks } : {}),
    ...(mentions.files.length > 0 ? { files: mentions.files } : {}),
  };
}

/** Names shown in a tooltip: the first few, then how many more there are. */
function namesDetail(names: readonly string[], shown = 30): string {
  const head = names.slice(0, shown).join(', ');
  return names.length > shown ? `${head}, … (+${names.length - shown})` : head;
}

/** One chip in the context row. A removed chip stays visible so it can be put back. */
export interface ContextChip {
  readonly id: RemovableChipId | 'file';
  readonly removed: boolean;
  readonly count?: number;
  readonly name?: string;
  /** What the chip contains, for its tooltip. */
  readonly detail?: string;
}

/** The chips for what could be sent: one per part that has content, plus mentioned files. */
export function contextChips(
  source: TerminalAssistSource,
  removed: ReadonlySet<RemovableChipId>,
  mentions: Mentions = NO_MENTIONS,
): ContextChip[] {
  const chip = (
    id: RemovableChipId,
    extra: { count?: number; name?: string; detail?: string } = {},
  ): ContextChip => ({
    id,
    removed: removed.has(id),
    ...extra,
  });
  return [
    chip('environment'),
    ...(source.listing.length > 0
      ? [chip('listing', { count: source.listing.length, detail: namesDetail(source.listing) })]
      : []),
    ...(source.selected.length > 0
      ? [chip('selection', { count: source.selected.length, detail: namesDetail(source.selected) })]
      : []),
    ...(source.otherFolder ? [chip('otherFolder', { detail: source.otherFolder })] : []),
    ...(source.blocks.length > 0
      ? [
          chip('blocks', {
            count: Math.min(source.blocks.length, MAX_CONTEXT_BLOCKS),
            detail: source.blocks
              .slice(-MAX_CONTEXT_BLOCKS)
              .map((block) => block.command)
              .join(', '),
          }),
        ]
      : []),
    ...mentions.files.map((name): ContextChip => ({ id: 'file', removed: false, name })),
  ];
}
