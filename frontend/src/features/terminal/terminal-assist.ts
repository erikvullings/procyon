import type { NoteProposal } from '../../models';

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

/** The person's answer to a request to create files (task 0338). */
export type TerminalWriteDecision = 'question' | 'session' | 'decline';

/** A file the assistant wants to create, waiting for approval (task 0338). */
export interface TerminalWriteRequest {
  /** The output folder as shown to the person, for example `./summaries/`. */
  readonly folder: string;
  readonly name: string;
  readonly bytes: number;
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
  /** The answer to the previous reply's `writeRequest`. */
  readonly writeDecision?: TerminalWriteDecision;
  /** Whether the assistant may offer to create files (the terminal setting). */
  readonly allowFileWrites?: boolean;
  /** Auto-approve mode (task 0356): files are created and edits applied without asking. */
  readonly autoApprove?: boolean;
  /** The active workspace, so `search_notes` may use the notes index (task 0337). */
  readonly workspaceId?: string;
  /** The session's trusted root: the folder it started in (task 0343). */
  readonly root?: string;
  /** A skill invoked with `/name`; its instructions go with the question (task 0343). */
  readonly skill?: string;
}

/** The paths under a session's root (folders end with `/`), for `@` completion (task 0343). */
export interface TerminalAssistFiles {
  readonly files: readonly string[];
  readonly truncated: boolean;
}

/** A skill `/` can invoke (task 0343). */
export interface TerminalAssistSkill {
  readonly name: string;
  readonly description: string;
  readonly source: 'project' | 'user' | 'note';
}

/** The assistant's answer: text, what it looked at, and a command it asks to run. */
export interface TerminalAssistReply {
  readonly text: string;
  readonly steps?: readonly string[];
  readonly runCommand?: string;
  /** A note the assistant proposes; it is reviewed before anything is saved (task 0337). */
  readonly noteProposal?: NoteProposal;
  /** Files the assistant wants to create; nothing is written until the person answers. */
  readonly writeRequest?: TerminalWriteRequest;
}

/**
 * Something shown while an answer is written: a finished line of it, a step taken, or what a long
 * tool call is doing now (it lasts until the next event).
 */
export type TerminalAssistEvent =
  | { readonly kind: 'line'; readonly text: string }
  | { readonly kind: 'step'; readonly text: string }
  | { readonly kind: 'progress'; readonly text: string };

/** A byte count for people: `812 B`, `1.2 KiB`. */
export function byteSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`;
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

/** A folder with more names than this has its listing left out until the person switches it on. */
export const LARGE_LISTING_NAMES = 64;

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
  for (const match of text.matchAll(/(?:^|\s)@(?:"([^"]+)"|'([^']+)'|(\S+))/g)) {
    const quoted = match[1] ?? match[2];
    const name = quoted ?? (match[3] ?? '').replace(/[,;:!?)\]]+$/, '').replace(/\.+$/, '');
    if (name !== '' && !files.includes(name)) files.push(name);
  }
  return { files };
}

/** What Tab types for an `@name`: delete `erase` typed characters, then type `insert`. */
export interface MentionCompletion {
  readonly erase: number;
  readonly insert: string;
}

/** A mention as it is typed: names with spaces are quoted, `@"My File.md"`; `@'My File.md'` is read too. */
export function mentionText(name: string): string {
  return /\s/.test(name) ? `@"${name}"` : `@${name}`;
}

/**
 * Tab completion of the `@name` at the end of what is typed, against the folder's names, like a
 * shell. A name with a space is completed in quotes (`@"My File.md"`); folders (listed with a
 * trailing `/`) are completed without a closing quote or space so the path can continue. With
 * several matches it extends to their common prefix. `undefined` when there is nothing to add.
 */
export function completeMention(
  typed: string,
  names: readonly string[],
): MentionCompletion | undefined {
  const match = /(?:^|\s)@(?:"([^"]*)|'([^']*)|(\S*))$/.exec(typed);
  if (match === null) return undefined;
  const quoted = match[1] !== undefined || match[2] !== undefined;
  // The quote the person opened is the one closed; `"` unless a name contains it.
  const mark = match[2] !== undefined ? "'" : '"';
  const partial = match[1] ?? match[2] ?? match[3] ?? '';
  const typedLength = partial.length + (quoted ? 1 : 0);
  const lower = partial.toLowerCase();
  const matches = names.filter((name) => name.toLowerCase().startsWith(lower));
  const first = matches[0];
  if (first === undefined) return undefined;
  if (matches.length === 1) {
    const folder = first.endsWith('/');
    const needsQuote = quoted || /\s/.test(first);
    if (!needsQuote)
      return { erase: 0, insert: `${first.slice(partial.length)}${folder ? '' : ' '}` };
    const rest = quoted ? first.slice(partial.length) : `${mark}${first}`;
    return { erase: quoted ? 0 : typedLength, insert: `${rest}${folder ? '' : `${mark} `}` };
  }
  let common = first;
  for (const name of matches) {
    let length = 0;
    while (length < common.length && common[length]?.toLowerCase() === name[length]?.toLowerCase())
      length += 1;
    common = common.slice(0, length);
  }
  // Without a quote the mention ends at whitespace, so stop before it.
  const space = common.search(/\s/);
  if (!quoted && space >= 0) common = common.slice(0, space);
  const insert = common.slice(partial.length);
  return insert === '' ? undefined : { erase: 0, insert };
}

/**
 * Tab completion of `@name` against every path under the session's root (task 0343): paths that
 * start with what is typed complete like a shell; otherwise a single path whose file or folder
 * name starts with it replaces it (`@main` becomes `@src/deep/main.rs`).
 */
export function completeMentionInTree(
  typed: string,
  paths: readonly string[],
): MentionCompletion | undefined {
  const match = /(?:^|\s)@(?:"([^"]*)|'([^']*)|(\S*))$/.exec(typed);
  if (match === null) return undefined;
  const quoted = match[1] !== undefined || match[2] !== undefined;
  const partial = match[1] ?? match[2] ?? match[3] ?? '';
  const lower = partial.toLowerCase();
  if (partial === '' || paths.some((path) => path.toLowerCase().startsWith(lower)))
    return completeMention(typed, paths);
  const byName = paths.filter((path) => {
    const name = path.replace(/\/$/, '').split('/').pop() ?? '';
    return name.toLowerCase().startsWith(lower);
  });
  const only = byName.length === 1 ? byName[0] : undefined;
  if (only === undefined) return undefined;
  const folder = only.endsWith('/');
  const text = mentionText(only);
  // A folder stays open so the path can continue; a quoted folder keeps its quote open.
  const insert = folder ? text.replace(/"$/, '') : `${text} `;
  return { erase: partial.length + (quoted ? 1 : 0) + 1, insert };
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

/**
 * The parts that are left out: the ones the person removed, plus the folder listing of a large
 * folder unless the person has switched that pill themselves (`touched`).
 */
export function effectiveRemoved(
  source: TerminalAssistSource,
  removed: ReadonlySet<RemovableChipId>,
  touched: ReadonlySet<RemovableChipId>,
): ReadonlySet<RemovableChipId> {
  if (source.listing.length <= LARGE_LISTING_NAMES || touched.has('listing')) return removed;
  return new Set([...removed, 'listing']);
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
