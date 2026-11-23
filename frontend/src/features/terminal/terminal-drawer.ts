import '@xterm/xterm/css/xterm.css';

import { FitAddon } from '@xterm/addon-fit';
import { type IDecoration, type IMarker, Terminal } from '@xterm/xterm';
import m, { type FactoryComponent } from 'mithril';
import {
  copyIcon,
  helpIcon,
  refreshIcon,
  sparklesIcon,
  wrenchIcon,
} from '../../components/tabler-icons';
import { t } from '../../i18n';
import type {
  DraftNoteRequest,
  LlmProfile,
  LlmRoleAssignment,
  Location,
  NoteProposal,
} from '../../models';
import type { TerminalShellIntegration } from '../../models/settings';
import {
  createAnswerStream,
  infoAnsi,
  noticeAnsi,
  questionAnsi,
  renderAnswerAnsi,
  stepsAnsi,
} from './answer-ansi';
import { type BlockTracker, type CommandBlock, createBlockTracker } from './block-tracker';
import { isReadOnlyCommand } from './read-only-command';
import { highlightShell } from './shell-highlight';
import { parseShellMark } from './shell-integration';
import {
  completeSlash,
  type ModelChoice,
  modelChoices,
  modelLines,
  parseSlash,
  pickModel,
  SLASH_COMMANDS,
  type SlashCommandName,
  slashNames,
  typedSlash,
} from './slash-commands';
import {
  assistContext,
  byteSize,
  type ContextChip,
  cleanCommand,
  commandFromAnswer,
  completeMention,
  completeMentionInTree,
  contextChips,
  EXPLAIN_REQUEST,
  effectiveRemoved,
  FIX_REQUEST,
  parseMentions,
  type RemovableChipId,
  type TerminalAssistContext,
  type TerminalAssistEvent,
  type TerminalAssistReply,
  type TerminalAssistSource,
  type TerminalToolResult,
  type TerminalWriteDecision,
  transcriptTail,
} from './terminal-assist';
import type { TerminalClient } from './terminal-client';
import { absolutePathOf, cdCommand } from './terminal-paths';
import { applyTypedInput, EMPTY_INPUT, suggestionRemainder, type TypedInput } from './typed-input';

/** What the file manager shows beside the terminal; the assistant may be told about it. */
export interface TerminalPaneContext {
  /** Names in the active pane's folder; folders end in `/`. */
  readonly listing: readonly string[];
  /** Names selected in the active pane. */
  readonly selected: readonly string[];
  /** The other pane's folder, as a location URI. */
  readonly otherFolder?: string | undefined;
}

export interface TerminalDrawerAttrs {
  readonly open: boolean;
  /** Composite `paneId:tabId` key the terminal is bound to; a tab keeps its terminal across
   * folder navigation and only loses it when the tab itself closes. */
  readonly tabKey: string | undefined;
  readonly location: Location | undefined;
  readonly client: TerminalClient;
  readonly onResize?: (height: number) => void;
  readonly onToggle: () => void;
  readonly onSwitchPane?: () => void;
  readonly onCycleTab?: (direction: 1 | -1) => void;
  readonly onFocusFolder?: () => void;
  readonly registerFocus?: (focus: () => boolean) => void;
  /** Lets the owner tear down a specific tab's terminal when that tab closes. */
  readonly registerDisposeTab?: (dispose: (tabKey: string) => void) => void;
  /** The active pane's listing and selection, read when the person asks the assistant (0336). */
  readonly paneContext?: () => TerminalPaneContext;
  /** The names in a folder (folders end with `/`), for `@name` completion in the terminal's folder. */
  /** Whether the terminal and the active pane keep to the same folder. */
  readonly followPane?: boolean;
  /** Whether a strictly read-only command the assistant proposes runs without asking. */
  readonly autoRunReadOnly?: boolean;
  /** Whether the assistant may offer to create files after approval (task 0338). */
  readonly allowFileWrites?: boolean;
  /** The terminal's shell on `host` (see `terminalHost`) moved to this absolute folder; the pane follows. */
  readonly onFollowFolder?: (host: string, path: string) => void;
  readonly listFolder?: (location: Location) => Promise<readonly string[]>;
  /** Whether the assistant is on (the Terminal setting); off leaves `#` to the shell (0336). */
  readonly assistantEnabled?: boolean;
  /** The person's choice for command-block shell integration (task 0333). */
  readonly shellIntegration?: TerminalShellIntegration;
  /** Hands the owner a way to type text at this drawer's active prompt (task 0333). */
  readonly registerInsert?: (insert: (text: string, host: string) => boolean) => void;
  /** The active workspace; its semantic consent lets the assistant search the notes index. */
  readonly workspaceId?: string | undefined;
  /** Hands the owner a way to save the active tab's session as a note (task 0337). */
  readonly registerSaveAsNote?: (save: () => boolean) => void;
  /** Turns the session into a draft note for review (task 0337). */
  readonly onSaveAsNote?: (request: DraftNoteRequest) => void;
  /** A note the assistant proposed, for review (task 0337). */
  readonly onNoteProposal?: (proposal: NoteProposal) => void;
  /** Saves the choice; `undefined` hides the consent prompt. */
  readonly onShellIntegrationChoice?: (choice: 'enabled' | 'disabled') => Promise<void>;
  /** The chat models `/model` lists and switches between (task 0343). */
  readonly models?: TerminalModels;
}

/** What `/model` needs from the owner: the saved endpoints and a way to switch the chat role. */
export interface TerminalModels {
  readonly list: () => Promise<{
    readonly profiles: readonly LlmProfile[];
    readonly chat: LlmRoleAssignment | null | undefined;
  }>;
  /** The models an endpoint reports; may be empty. */
  readonly discover: (profileId: string) => Promise<readonly string[]>;
  readonly select: (choice: ModelChoice) => Promise<void>;
}

/** A listing kept for completion until something may have changed it. */
interface CompletionCache {
  readonly root: string;
  readonly names: readonly string[];
  stale: boolean;
}

/**
 * The machine a location's shell runs on: `local`, or `sftp://<connection-id>` for one SSH
 * connection. A tab keeps its terminal across folder navigation, but not across hosts.
 */
export function terminalHost(location: Location): string {
  const match = /^sftp:\/\/[^/]+/.exec(location.uri);
  return match?.[0] ?? 'local';
}

/** What `/help` says about each command. */
function slashHelp(command: SlashCommandName): string {
  switch (command) {
    case 'new':
      return t('terminal', 'slashHelpNew');
    case 'clear':
      return t('terminal', 'slashHelpClear');
    case 'compact':
      return t('terminal', 'slashHelpCompact');
    case 'model':
      return t('terminal', 'slashHelpModel');
    case 'help':
      return t('terminal', 'slashHelpHelp');
  }
}

const IS_MAC = typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent);

/**
 * The label of an assistant chord for a letter: `Ctrl+Shift+E`, or `⌘⇧E` on a Mac, where Cmd+Shift
 * works as well as the physical Control key (Control stays as it is for the shell's own use).
 */
function chordLabel(letter: string): string {
  return IS_MAC ? `⌘⇧${letter}` : `Ctrl+Shift+${letter}`;
}

/** Most command blocks kept per terminal; older blocks lose their markers and actions. */
const MAX_BLOCKS = 200;
/** Most commands remembered for suggestions in one terminal session. */
const MAX_HISTORY = 500;

type LiveTerminal = {
  readonly key: string;
  readonly host: string;
  readonly terminal: Terminal;
  readonly fit: FitAddon;
  readonly surface: HTMLElement;
  readonly blocks: BlockTracker<IMarker>;
  readonly decorations: Map<number, IDecoration>;
  sessionId?: string | undefined;
  opening?: Promise<void> | undefined;
  /** Identifies the session being opened; events and ids from any other open are ignored. */
  openToken?: object | undefined;
  /** What the person has typed on the current shell line. */
  typed: TypedInput;
  /** The folder the pane and the shell were last known to share; see `followPane`. */
  folder: string | undefined;
  /** Commands run in this session, oldest first, for suggestions. */
  history: string[];
  /** The `#` questions asked in this tab, oldest first, for Up/Down recall. */
  questions: string[];
  /** The question shown by recall (an index into `questions`), `undefined` when not recalling. */
  recallIndex: number | undefined;
  /** What was typed before recall started, brought back by Down past the newest question. */
  recallDraft: string;
  /** The command the person just ran; becomes the next block's command. */
  pendingCommand: string;
  /** Whether the shell is waiting for input (true before any shell mark is seen). */
  atPrompt: boolean;
  /** Whether the shell reports command marks; without them the terminal shows setup help. */
  markSeen: boolean;
  /** The rest of the suggested command, when one matches what is typed. */
  suggestion: string | undefined;
  /** Identifies this tab's assistant conversation in the host (task 0336). */
  readonly assistSession: string;
  /** Context parts the person removed with a chip; they stay removed for this tab. */
  readonly removedChips: Set<RemovableChipId>;
  /** Called with the block of the command the assistant started once it finishes. */
  awaiting: ((block: CommandBlock<IMarker>) => void) | undefined;
  /** Whether this tab has an assistant conversation, which keeps the assistant bar visible. */
  assistActive: boolean;
  /** Keys typed while the assistant is answering; sent to the shell once its prompt is back. */
  held: string[];
  /** Counts requests, so the answer to a dismissed one is dropped. */
  requestId: number;
  /** A half-typed command set aside while the assistant answers, put back afterwards. */
  restoreLine: TypedInput | undefined;
  /** Pills the person has switched themselves, so the large-folder default no longer applies. */
  readonly touchedChips: Set<RemovableChipId>;
  /** Whether the shell is ready for input. Keys typed earlier wait, or they would be echoed twice. */
  ready: boolean;
  /** Keys typed before the shell was ready. */
  pendingInput: string[];
  readyTimer: ReturnType<typeof setTimeout> | undefined;
  /** The session's trusted root: the folder of its first question or of `/new` (task 0343). */
  assistRoot: Location | undefined;
  /** The paths under the root, for `@` completion. */
  fileCache: CompletionCache | undefined;
  /** The skill names for `/` completion. */
  skillCache: CompletionCache | undefined;
  /** The models `/model` listed last, so `/model 2` picks from what was shown. */
  models: readonly ModelChoice[];
};

export interface TerminalKeyHandlers {
  readonly onToggle: () => void;
  readonly onSwitchPane?: () => void;
  readonly onCycleTab?: (direction: 1 | -1) => void;
  readonly onFocusFolder?: () => void;
  /** Accepts the shown suggestion. Returns whether one was accepted. */
  readonly onAcceptSuggestion?: () => boolean;
  /** The assistant's and block keys (Enter, Ctrl+E, Ctrl+C, ...). Returns whether it used the key. */
  readonly onAssistantKey?: (event: KeyboardEvent) => boolean;
  /** Completes the `@name` being typed after `#`. Returns whether it used the Tab. */
  readonly onCompleteMention?: () => boolean;
  /** Up/Down on a `#` line recalls earlier questions. Returns whether it used the key. */
  readonly onRecallQuestion?: (direction: 'older' | 'newer') => boolean;
  /** Sends line-editing bytes (word and line jumps and deletes) to the shell. */
  readonly onShellInput?: (data: string) => void;
}

/**
 * The shell bytes for the line-editing chords xterm.js does not send by itself, so word and line
 * jumps and deletes behave as in a native terminal: Option/Ctrl+Left/Right move by word,
 * Cmd+Left/Right go to the start/end of the line, Option/Ctrl+Backspace delete the word before the
 * cursor, Cmd+Backspace deletes to the start of the line, Option+Delete the word after it and
 * Cmd+Delete to the end of the line. `undefined` for every other key.
 */
export function lineEditSequence(event: KeyboardEvent): string | undefined {
  if (event.shiftKey) return undefined;
  const word = (event.altKey || event.ctrlKey) && !event.metaKey;
  const line = event.metaKey && !event.altKey && !event.ctrlKey;
  if (!word && !line) return undefined;
  switch (event.key) {
    case 'ArrowLeft':
      return word ? '\x1bb' : '\x01';
    case 'ArrowRight':
      return word ? '\x1bf' : '\x05';
    case 'Backspace':
      return word ? (event.ctrlKey ? '\x17' : '\x1b\x7f') : '\x15';
    case 'Delete':
      return word ? '\x1bd' : '\x0b';
    default:
      return undefined;
  }
}

/**
 * Returns file-manager navigation chords to the application while xterm owns DOM focus.
 *
 * Plain Tab / Shift+Tab are deliberately left alone (`return true`) so xterm forwards them
 * to the PTY for shell auto-completion. Pane/tab navigation instead requires a modifier
 * (Ctrl+Tab to cycle tabs, Alt+Tab to switch panes) so it never steals the shell's Tab key.
 * Right arrow accepts a shown suggestion and is otherwise left to the shell.
 */
export function handleTerminalKeyEvent(
  event: KeyboardEvent,
  handlers: TerminalKeyHandlers,
): boolean {
  const bareModifiers = !event.altKey && !event.metaKey && !event.shiftKey;
  const toggleKey =
    bareModifiers &&
    ((!event.ctrlKey && (event.key === 'F12' || event.code === 'F12')) ||
      (event.ctrlKey && (event.key === '`' || event.code === 'Backquote')));
  if (event.type !== 'keydown') return true;
  if (handlers.onAssistantKey?.(event) === true) {
    event.preventDefault();
    event.stopPropagation();
    return false;
  }
  if (bareModifiers && !event.ctrlKey && event.key === 'ArrowRight') {
    if (handlers.onAcceptSuggestion?.() === true) {
      event.preventDefault();
      event.stopPropagation();
      return false;
    }
  }
  if (
    bareModifiers &&
    !event.ctrlKey &&
    (event.key === 'ArrowUp' || event.key === 'ArrowDown') &&
    handlers.onRecallQuestion?.(event.key === 'ArrowUp' ? 'older' : 'newer') === true
  ) {
    event.preventDefault();
    event.stopPropagation();
    return false;
  }
  const edit = lineEditSequence(event);
  if (edit !== undefined && handlers.onShellInput !== undefined) {
    handlers.onShellInput(edit);
    event.preventDefault();
    event.stopPropagation();
    return false;
  }
  if (
    event.key === 'Tab' &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.metaKey &&
    !event.shiftKey &&
    handlers.onCompleteMention?.() === true
  ) {
    event.preventDefault();
    event.stopPropagation();
    return false;
  }
  const tabKey = event.key === 'Tab' || event.code === 'Tab';
  const navTabKey = tabKey && !event.metaKey && (event.ctrlKey || event.altKey);
  const handled = toggleKey || navTabKey;
  if (!handled) return true;
  event.preventDefault();
  event.stopPropagation();
  if (toggleKey) handlers.onToggle();
  else if (navTabKey) {
    if (event.ctrlKey) handlers.onCycleTab?.(event.shiftKey ? -1 : 1);
    else if (event.shiftKey) handlers.onFocusFolder?.();
    else handlers.onSwitchPane?.();
  }
  return false;
}

const ANSI_COLORS = [
  ['red', '--fm-error'],
  ['green', '--fm-success'],
  ['yellow', '--fm-warning'],
  ['blue', '--fm-accent'],
] as const;

/**
 * Theme-aware ANSI colours so `ls --color`, git, compiler output and shell-side syntax
 * highlighters (zsh-syntax-highlighting, fish) stay legible in both light and dark themes.
 * Colours come from the app's semantic tokens; anything unresolved falls back to xterm's default.
 */
export function ansiPalette(style: CSSStyleDeclaration): Record<string, string> {
  const palette: Record<string, string> = {};
  for (const [name, token] of ANSI_COLORS) {
    const value = style.getPropertyValue(token).trim();
    if (value === '') continue;
    palette[name] = value;
    palette[`bright${name[0]?.toUpperCase()}${name.slice(1)}`] = value;
  }
  const muted = style.getPropertyValue('--fm-text-muted').trim();
  if (muted !== '') palette.brightBlack = muted;
  return palette;
}

/** Shows one location-owned terminal surface without confusing it with the shared drawer host. */
export function showTerminalSurface(host: HTMLElement, surface: HTMLElement): void {
  if (host.childElementCount !== 1 || host.firstElementChild !== surface) {
    host.replaceChildren(surface);
  }
}

/** The text of one terminal line range, as plain text. */
function linesText(terminal: Terminal, first: number, end: number): string[] {
  const buffer = terminal.buffer.active;
  const lines: string[] = [];
  for (let row = Math.max(0, first); row < end && row < buffer.length; row += 1) {
    lines.push(buffer.getLine(row)?.translateToString(true) ?? '');
  }
  return lines;
}

/** The newest visible output of a terminal, for when the shell reports no command blocks. */
function scrollbackTranscript(live: LiveTerminal): string {
  return transcriptTail(linesText(live.terminal, 0, live.terminal.buffer.active.length));
}

/** The text of the terminal's cursor line (the line being edited), without its prompt-free trailing blanks. */
function cursorLine(terminal: Terminal): string {
  const buffer = terminal.buffer.active;
  let row = buffer.baseY + buffer.cursorY;
  let text = buffer.getLine(row)?.translateToString(true) ?? '';
  // A long line wraps onto several rows; the whole logical line is the input.
  while (buffer.getLine(row)?.isWrapped === true && row > 0) {
    row -= 1;
    text = (buffer.getLine(row)?.translateToString(false) ?? '') + text;
  }
  return text;
}

/**
 * The question on the line being submitted, if it is one (`#` at the start of the line, or after
 * the prompt's last character). The tracked input is used when it is known; otherwise, after
 * editing keys such as arrows, the line as shown on screen is read. Returns `undefined` when the
 * line is not a question; `''` for a bare `#`.
 */
export function questionAtPrompt(typed: TypedInput, line: string): string | undefined {
  if (typed.known && typed.text.startsWith('#')) return typed.text.slice(1).trim();
  const match = /(?:^|[%$>❯»]\s+)#\s*(.*)$/.exec(line.trimEnd());
  return match === null ? undefined : (match[1] ?? '').trim();
}

/** Shell text as coloured spans, so commands, options and strings stand apart in the overlay. */
function highlightedShell(text: string): m.Children {
  return highlightShell(text).map((token) =>
    token.kind === 'plain' ? token.text : m(`span.fm-shell-${token.kind}`, token.text),
  );
}

/** Maps a failure from the desktop host to the message the person sees. */
function assistFailureText(reason: string): string {
  switch (reason) {
    case 'roleNotAssigned':
      return t('terminal', 'assistRoleNotAssigned');
    case 'consentRequired':
      return t('terminal', 'assistConsentRequired');
    case 'empty':
      return t('terminal', 'assistEmpty');
    case 'noOutput':
      return t('terminal', 'assistNoOutput');
    default:
      return t('terminal', 'assistFailed');
  }
}

/**
 * What the assistant bar shows for one tab besides the pills: a question being answered, a
 * command to insert, a command waiting for approval, or an approved command that is running. The
 * conversation itself is written into the terminal.
 */
type AssistPanel =
  | { readonly status: 'busy'; readonly tabKey: string }
  | { readonly status: 'suggest'; readonly tabKey: string; readonly command: string }
  | { readonly status: 'approve'; readonly tabKey: string; readonly command: string }
  | { readonly status: 'running'; readonly tabKey: string; readonly command: string }
  | {
      readonly status: 'write';
      readonly tabKey: string;
      readonly folder: string;
      readonly name: string;
      readonly bytes: number;
    };

/** A bottom drawer whose xterm instances remain mounted and keyed by backing location. */
export const TerminalDrawer: FactoryComponent<TerminalDrawerAttrs> = () => {
  const terminals = new Map<string, LiveTerminal>();
  let observer: ResizeObserver | undefined;
  let currentAttrs: TerminalDrawerAttrs | undefined;
  let panel: AssistPanel | undefined;
  /** The command being edited with Ctrl+E before it is inserted; `undefined` when not editing. */
  let editing: string | undefined;
  /** Whether the assistant help dialog is open. */
  let helpOpen = false;
  /** Whether the pills are unfolded while the bar is too narrow to show them in a row. */
  let contextOpen = false;
  /** The tooltip being shown, and the timer that will show it. Drawn by the drawer itself: the
   * webview shows no native tooltip for these buttons. */
  let tip:
    | { readonly text: string; readonly x: number; readonly y: number; readonly place: TipPlace }
    | undefined;
  let tipTimer: ReturnType<typeof setTimeout> | undefined;

  type TipPlace = 'above-left' | 'below-right';

  function hideTip(): void {
    clearTimeout(tipTimer);
    tipTimer = undefined;
    if (tip !== undefined) {
      tip = undefined;
      m.redraw();
    }
  }

  /** Mouse and keyboard-focus handlers that show `text()` near the element after a short delay. */
  function tooltipEvents(text: () => string, place: TipPlace) {
    const show = (event: Event) => {
      const element = event.currentTarget as HTMLElement;
      clearTimeout(tipTimer);
      tipTimer = setTimeout(() => {
        const box = element.getBoundingClientRect();
        tip = {
          text: text(),
          x: place === 'above-left' ? box.left : box.right,
          y: place === 'above-left' ? box.top : box.bottom,
          place,
        };
        m.redraw();
      }, 500);
    };
    return {
      onmouseenter: show,
      onfocus: show,
      onmouseleave: hideTip,
      onblur: hideTip,
      onmousedown: hideTip,
    };
  }

  /** The parts left out of the next question: removed pills and, for a large folder, the listing. */
  const removedFor = (live: LiveTerminal, source: TerminalAssistSource) =>
    effectiveRemoved(source, live.removedChips, live.touchedChips);

  /**
   * The shell can take input. Keys typed before that wait: the terminal would echo them once
   * before the shell starts and the shell would show them again at its prompt.
   */
  function markReady(live: LiveTerminal): void {
    if (live.ready) return;
    clearTimeout(live.readyTimer);
    if (live.sessionId === undefined) {
      live.readyTimer = setTimeout(() => markReady(live), 50);
      return;
    }
    live.ready = true;
    live.readyTimer = undefined;
    for (const data of live.pendingInput.splice(0)) onTypedData(live, data);
  }

  /** Waits `delay` ms for the shell's prompt mark, then treats the shell as ready regardless. */
  function armReady(live: LiveTerminal, delay: number): void {
    if (live.ready) return;
    clearTimeout(live.readyTimer);
    live.readyTimer = setTimeout(() => markReady(live), delay);
  }

  const assistantOn = (): boolean => currentAttrs?.assistantEnabled !== false;

  const isBusy = (live: LiveTerminal): boolean =>
    panel?.status === 'busy' && panel.tabKey === live.key;

  /** What the assistant could be told about this tab, up to a block when asked about an old one. */
  function assistSource(
    live: LiveTerminal,
    upTo?: CommandBlock<IMarker>,
    scrollback = false,
  ): TerminalAssistSource {
    const pane = currentAttrs?.paneContext?.();
    const finished = live.blocks.blocks;
    const included = upTo === undefined ? finished : finished.slice(0, finished.indexOf(upTo) + 1);
    const blocks = included.map((block) => ({
      command: block.command,
      exitCode: block.exitCode,
      output: blockOutput(live, block),
    }));
    // Without command blocks the visible output stands in for the last command.
    const visible = scrollback && blocks.length === 0 ? scrollbackTranscript(live) : '';
    return {
      listing: pane?.listing ?? [],
      selected: pane?.selected ?? [],
      otherFolder: pane?.otherFolder,
      blocks:
        visible === '' ? blocks : [{ command: '(visible output)', exitCode: 0, output: visible }],
    };
  }

  /**
   * Asks the session a question. The question is written into the terminal, in place of the
   * prompt line, and the answer follows it when it arrives. Typing is held until then.
   */
  function askAssistant(
    live: LiveTerminal,
    question: string,
    options: {
      readonly upTo?: CommandBlock<IMarker>;
      readonly scrollback?: boolean;
      /** What to show as the question; defaults to the question itself. */
      readonly display?: string;
      /** Set a half-typed command aside and put it back after the answer. */
      readonly keepTypedLine?: boolean;
      /** A skill invoked with `/name` (task 0343). */
      readonly skill?: string;
    } = {},
  ): void {
    if (isBusy(live)) return;
    live.assistActive = true;
    if (currentAttrs?.location !== undefined) live.assistRoot ??= currentAttrs.location;
    live.held = [];
    if (options.keepTypedLine === true && (live.typed.text !== '' || !live.typed.known)) {
      writeToShell(live, '\x15');
      live.restoreLine = live.typed;
    }
    live.typed = EMPTY_INPUT;
    live.suggestion = undefined;
    live.terminal.write(`\r\x1b[2K${questionAnsi(options.display ?? question)}`);
    sendToSession(live, {
      question,
      ...(options.skill === undefined ? {} : { skill: options.skill }),
      context: assistContext(
        assistSource(live, options.upTo, options.scrollback),
        removedFor(live, assistSource(live, options.upTo, options.scrollback)),
        parseMentions(question),
      ),
    });
  }

  /** Sends a question or the result of an approved command to this tab's session. */
  function sendToSession(
    live: LiveTerminal,
    input: {
      readonly question?: string;
      readonly context?: TerminalAssistContext;
      readonly toolResult?: TerminalToolResult;
      readonly writeDecision?: TerminalWriteDecision;
      readonly skill?: string;
    },
  ): void {
    const attrs = currentAttrs;
    if (attrs?.location === undefined) return;
    live.requestId += 1;
    const token = live.requestId;
    panel = { status: 'busy', tabKey: live.key };
    editing = undefined;
    m.redraw();
    // Lines and steps are written as they arrive; the reply then only says what to offer next.
    const answer = createAnswerStream(live.terminal.cols);
    let streamed = false;
    const onEvent = (event: TerminalAssistEvent): void => {
      if (token !== live.requestId) return;
      streamed = true;
      live.terminal.write(
        event.kind === 'step' ? stepsAnsi([event.text]) : answer.line(event.text),
      );
    };
    attrs.client
      .assist(
        {
          kind: 'ask',
          transcript: '',
          workingDirectory: attrs.location.uri,
          sessionId: live.assistSession,
          ...(attrs.workspaceId === undefined ? {} : { workspaceId: attrs.workspaceId }),
          ...(input.question === undefined ? {} : { request: input.question }),
          ...(input.context === undefined ? {} : { context: input.context }),
          ...(input.toolResult === undefined ? {} : { toolResult: input.toolResult }),
          ...(input.writeDecision === undefined ? {} : { writeDecision: input.writeDecision }),
          ...(input.skill === undefined ? {} : { skill: input.skill }),
          ...(live.assistRoot === undefined ? {} : { root: live.assistRoot.uri }),
          allowFileWrites: attrs.allowFileWrites !== false,
        },
        onEvent,
      )
      .then((reply) => {
        // The assistant may have created files or run commands.
        markStale(live);
        presentReply(live, token, reply, streamed);
      })
      .catch((error: unknown) =>
        present(live, token, noticeAnsi(assistFailureText(String(error))), undefined),
      )
      .finally(() => m.redraw());
  }

  /** Writes a reply into the terminal; a command in it goes to the bar for the person to act on. */
  function presentReply(
    live: LiveTerminal,
    token: number,
    reply: TerminalAssistReply,
    streamed: boolean,
  ): void {
    // A streamed answer is already in the terminal; only a host that cannot stream gets it now.
    let text = streamed ? '' : stepsAnsi(reply.steps ?? []);
    let next: AssistPanel | undefined;
    if (reply.noteProposal !== undefined && token === live.requestId) {
      text += noticeAnsi(t('notes', 'proposalShown'));
      currentAttrs?.onNoteProposal?.(reply.noteProposal);
    }
    let autoRun: string | undefined;
    if (reply.writeRequest !== undefined) {
      next = { status: 'write', tabKey: live.key, ...reply.writeRequest };
    } else if (reply.runCommand !== undefined) {
      const command = cleanCommand(reply.runCommand);
      // A read-only command runs at once, unless the person has started typing meanwhile.
      if (
        currentAttrs?.autoRunReadOnly !== false &&
        isReadOnlyCommand(command) &&
        live.markSeen &&
        live.typed.known &&
        live.typed.text === ''
      ) {
        autoRun = command;
      } else {
        next = { status: 'approve', tabKey: live.key, command };
      }
    } else {
      if (!streamed) text += renderAnswerAnsi(reply.text, live.terminal.cols);
      const command = commandFromAnswer('ask', reply.text);
      if (command !== undefined) {
        next = { status: 'suggest', tabKey: live.key, command: cleanCommand(command) };
      }
    }
    present(live, token, text, next, autoRun);
  }

  /** Shows `text` in the terminal, then brings the shell prompt back. Dropped if dismissed. */
  function present(
    live: LiveTerminal,
    token: number,
    text: string,
    next: AssistPanel | undefined,
    thenRun?: string,
  ): void {
    if (token !== live.requestId) return;
    panel = next;
    const done = () => {
      releasePrompt(live);
      if (thenRun !== undefined && token === live.requestId) runApproved(live, thenRun);
    };
    if (text === '') done();
    else live.terminal.write(`\r\x1b[2K${text}`, done);
  }

  /** Asks the shell for a fresh prompt, puts back a half-typed command, and sends held keys. */
  function releasePrompt(live: LiveTerminal): void {
    writeToShell(live, '\r');
    if (live.restoreLine !== undefined) {
      writeToShell(live, '\x19');
      live.typed = live.restoreLine;
      live.restoreLine = undefined;
    }
    for (const data of live.held.splice(0)) onTypedData(live, data);
    m.redraw();
  }

  /** Stops waiting for the answer; it is dropped when it arrives. */
  function cancelBusy(live: LiveTerminal): void {
    live.requestId += 1;
    void currentAttrs?.client.assistCancel?.(live.assistSession).catch(() => undefined);
    panel = undefined;
    live.terminal.write('\r\x1b[2K', () => releasePrompt(live));
    m.redraw();
  }

  /** Runs the approved command in the terminal; its block goes back to the assistant. */
  function runApproved(live: LiveTerminal, command: string): void {
    // One approval runs one line; a newline or control byte would run more.
    // biome-ignore lint/suspicious/noControlCharactersInRegex: matching control bytes is the point
    if (live.sessionId === undefined || /[\u0000-\u001f\u007f]/.test(command)) return;
    panel = { status: 'running', tabKey: live.key, command };
    const waitForResult = (block: CommandBlock<IMarker>): void => {
      // The next finished block may be something the person typed meanwhile; keep waiting.
      if (block.command !== command) {
        live.awaiting = waitForResult;
        return;
      }
      sendToSession(live, {
        toolResult: {
          command,
          exitCode: block.exitCode,
          output: transcriptTail(blockOutput(live, block).split('\n')),
        },
      });
    };
    live.awaiting = waitForResult;
    live.pendingCommand = command;
    live.history.push(command);
    // Ctrl+U first, so the command never runs appended to something half typed.
    writeToShell(live, `\x15${command}\r`);
    m.redraw();
  }

  /**
   * Asks the owner to turn this tab's session into a note: the assistant conversation and the
   * finished commands (or, without command blocks, the visible output). Returns whether there was
   * a terminal to save.
   */
  function saveAsNote(live: LiveTerminal | undefined): boolean {
    const attrs = currentAttrs;
    if (live === undefined || attrs?.location === undefined || attrs.onSaveAsNote === undefined) {
      return false;
    }
    attrs.onSaveAsNote({
      ...(live.assistActive ? { sessionId: live.assistSession } : {}),
      blocks: assistSource(live, undefined, true).blocks ?? [],
      workingDirectory: attrs.location.uri,
    });
    return true;
  }

  /** Answers the request to create files; declining makes the assistant stop. */
  function answerWrite(live: LiveTerminal, decision: TerminalWriteDecision): void {
    if (decision !== 'decline') {
      sendToSession(live, { writeDecision: decision });
      return;
    }
    panel = undefined;
    void currentAttrs?.client
      .assist({
        kind: 'ask',
        transcript: '',
        workingDirectory: currentAttrs.location?.uri ?? '',
        sessionId: live.assistSession,
        writeDecision: 'decline',
      })
      .catch(() => undefined);
    m.redraw();
  }

  /** Tells the session the person declined; the assistant stops. */
  function declineApproved(live: LiveTerminal, command: string): void {
    panel = undefined;
    void currentAttrs?.client
      .assist({
        kind: 'ask',
        transcript: '',
        workingDirectory: currentAttrs.location?.uri ?? '',
        sessionId: live.assistSession,
        toolResult: { command, declined: true },
      })
      .catch(() => undefined);
    m.redraw();
  }

  /** Asks about a block, or the last one; without command blocks the visible output is used. */
  function askAboutBlock(
    live: LiveTerminal,
    request: string,
    label: string,
    block?: CommandBlock<IMarker>,
  ): void {
    const target = block ?? live.blocks.blocks.at(-1);
    askAssistant(live, request, {
      display: label,
      keepTypedLine: true,
      ...(target === undefined ? { scrollback: true } : { upTo: target }),
    });
  }

  function explain(live: LiveTerminal, block?: CommandBlock<IMarker>): void {
    askAboutBlock(live, EXPLAIN_REQUEST, t('terminal', 'blockExplain'), block);
  }

  function fix(live: LiveTerminal, block?: CommandBlock<IMarker>): void {
    askAboutBlock(live, FIX_REQUEST, t('terminal', 'blockFix'), block);
  }

  /** Ends a tab's terminal and forgets its assistant conversation. */
  function disposeLive(live: LiveTerminal): void {
    live.terminal.dispose();
    live.awaiting = undefined;
    void currentAttrs?.client.assistClose(live.assistSession).catch(() => undefined);
  }

  /** Types a command at the prompt without pressing Enter: the person runs it. */
  function insertCommand(live: LiveTerminal, command: string): void {
    const attrs = currentAttrs;
    if (attrs === undefined || live.sessionId === undefined) return;
    void attrs.client.write(live.sessionId, new TextEncoder().encode(command));
    live.terminal.focus();
    panel = undefined;
    editing = undefined;
    m.redraw();
  }

  /**
   * Types `text` at the active tab's prompt, without Enter, when that terminal runs on `host`.
   * Returns whether the text was typed.
   */
  function insertAtPrompt(text: string, host: string): boolean {
    const key = currentAttrs?.tabKey;
    const live = key === undefined ? undefined : terminals.get(key);
    if (live === undefined || live.sessionId === undefined || live.host !== host) return false;
    writeToShell(live, text);
    live.typed = { text: `${live.typed.text}${text}`, known: live.typed.known };
    live.terminal.focus();
    m.redraw();
    return true;
  }

  function writeToShell(live: LiveTerminal, text: string): void {
    const attrs = currentAttrs;
    if (attrs === undefined || live.sessionId === undefined) return;
    void attrs.client.write(live.sessionId, new TextEncoder().encode(text));
  }

  function copyText(text: string): void {
    void navigator.clipboard.writeText(text).catch(() => undefined);
  }

  /** Records a shell mark; a finished command becomes a block with its own actions. */
  /** The shell reported its folder: the pane follows, unless it is already there. */
  function shellFolderChanged(live: LiveTerminal, path: string): void {
    if (live.folder === path) return;
    live.folder = path;
    const attrs = currentAttrs;
    if (attrs?.followPane === true && attrs.location !== undefined) {
      attrs.onFollowFolder?.(terminalHost(attrs.location), path);
    }
  }

  /**
   * The pane moved to another folder: a shell waiting at an empty prompt follows with `cd`. The
   * terminal is never touched while a program runs or something is typed, and the folder is
   * remembered either way so the two are not compared again until the pane moves.
   */
  function paneFolderChanged(live: LiveTerminal, attrs: TerminalDrawerAttrs): void {
    const path = attrs.location === undefined ? undefined : absolutePathOf(attrs.location.uri);
    if (path === undefined || !path.startsWith('/')) return;
    const previous = live.folder;
    live.folder = path;
    if (previous === undefined || previous === path || attrs.followPane !== true) return;
    const idle =
      live.ready &&
      live.sessionId !== undefined &&
      live.markSeen &&
      live.atPrompt &&
      !isBusy(live) &&
      live.typed.known &&
      live.typed.text === '';
    if (idle) writeToShell(live, `${cdCommand(path)}\r`);
  }

  function onShellMark(live: LiveTerminal, payload: string): void {
    const mark = parseShellMark(payload);
    if (mark === undefined) return;
    if (mark.kind === 'cwd') {
      shellFolderChanged(live, mark.path);
      return;
    }
    live.markSeen = true;
    markReady(live);
    if (mark.kind === 'prompt') live.atPrompt = true;
    if (mark.kind === 'output') live.atPrompt = false;
    const block = live.blocks.apply(
      mark,
      () => live.terminal.registerMarker(0),
      () => live.pendingCommand,
    );
    if (block === undefined) return;
    live.atPrompt = true;
    // The strip offers Explain and Fix according to the newest block.
    m.redraw();
    const awaiting = live.awaiting;
    live.awaiting = undefined;
    awaiting?.(block);
    const decoration = live.terminal.registerDecoration({
      marker: block.promptMark,
      anchor: 'right',
      x: 0,
      width: 8,
      height: 1,
    });
    if (decoration === undefined) return;
    live.decorations.set(block.id, decoration);
    decoration.onRender((element) => {
      m.render(element, blockActions(live, block));
    });
  }

  /** The actions shown on a finished block's prompt line. */
  /** The output of a finished block, as plain text. */
  function blockOutput(live: LiveTerminal, block: CommandBlock<IMarker>): string {
    return linesText(live.terminal, block.outputMark.line, block.endMark.line).join('\n');
  }

  function blockActions(live: LiveTerminal, block: CommandBlock<IMarker>): m.Children {
    const failed = block.exitCode !== 0;
    const action = (label: string, shortcut: string, icon: m.Children, onclick: () => void) =>
      m(
        'button.fm-terminal-block-action',
        {
          type: 'button',
          title: `${label} (${shortcut})`,
          'aria-label': label,
          'aria-keyshortcuts': shortcut,
          onclick,
        },
        icon,
      );
    return m('.fm-terminal-block', [
      m('span.fm-terminal-block-status', {
        class: failed ? 'fm-terminal-block-status--failed' : 'fm-terminal-block-status--ok',
        title: failed ? t('terminal', 'blockFailed') : t('terminal', 'blockSucceeded'),
      }),
      assistantOn()
        ? action(t('terminal', 'blockExplain'), chordLabel('E'), sparklesIcon({ size: 14 }), () =>
            explain(live, block),
          )
        : undefined,
      assistantOn() && failed
        ? action(t('terminal', 'blockFix'), chordLabel('F'), wrenchIcon({ size: 14 }), () =>
            fix(live, block),
          )
        : undefined,
      action(t('terminal', 'blockCopy'), chordLabel('C'), copyIcon({ size: 14 }), () =>
        copyText(blockOutput(live, block)),
      ),
      action(t('terminal', 'blockRerun'), chordLabel('R'), refreshIcon({ size: 14 }), () =>
        rerunBlock(live, block),
      ),
    ]);
  }

  function rerunBlock(live: LiveTerminal, block: CommandBlock<IMarker>): void {
    if (block.command !== '') writeToShell(live, `${block.command}\r`);
  }

  /**
   * Keys owned by the assistant and the command blocks (task 0333). While an answer is shown,
   * Enter inserts it, Ctrl+E edits it and Ctrl+C or Escape rejects it; those keys reach the shell
   * again once the answer is gone. Ctrl+Shift+E/F/C/R/S (Cmd+Shift on a Mac too) act on the last command block or the session. Alt+Y and
   * Alt+N answer the shell-integration prompt. Returns whether the key was used.
   */
  function assistantKey(event: KeyboardEvent): boolean {
    const attrs = currentAttrs;
    const key = attrs?.tabKey;
    const live = key === undefined ? undefined : terminals.get(key);
    const name = event.key.toLowerCase();
    const plain = !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    const ctrl = event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    const ctrlShift =
      event.shiftKey &&
      !event.altKey &&
      (IS_MAC ? event.ctrlKey !== event.metaKey : event.ctrlKey && !event.metaKey);

    if (
      event.altKey &&
      !event.ctrlKey &&
      !event.metaKey &&
      (name === 'y' || name === 'n') &&
      attrs?.shellIntegration === 'unset' &&
      attrs.onShellIntegrationChoice !== undefined
    ) {
      chooseShellIntegration(live, name === 'y' ? 'enabled' : 'disabled');
      return true;
    }

    const shown = panel !== undefined && panel.tabKey === key ? panel : undefined;
    if (shown?.status === 'busy' && live !== undefined) {
      if ((plain && event.key === 'Escape') || (ctrl && name === 'c')) {
        cancelBusy(live);
        return true;
      }
    }
    // Enter belongs to the shell while something is typed (for example the next # question).
    const lineEmpty = live !== undefined && live.typed.text === '';
    if (shown?.status === 'approve' && live !== undefined) {
      if (plain && event.key === 'Enter' && lineEmpty) {
        if (live.markSeen) runApproved(live, shown.command);
        else insertCommand(live, shown.command);
        return true;
      }
      if ((plain && event.key === 'Escape') || (ctrl && name === 'c')) {
        declineApproved(live, shown.command);
        return true;
      }
    }
    if (shown?.status === 'write' && live !== undefined) {
      if (plain && event.key === 'Enter' && lineEmpty) {
        answerWrite(live, 'question');
        return true;
      }
      if ((plain && event.key === 'Escape') || (ctrl && name === 'c')) {
        answerWrite(live, 'decline');
        return true;
      }
    }
    if (shown?.status === 'running' && live !== undefined && plain && event.key === 'Escape') {
      live.awaiting = undefined;
      rejectAnswer();
      return true;
    }
    if (shown?.status === 'suggest' && live !== undefined) {
      if (plain && event.key === 'Enter' && lineEmpty) {
        insertCommand(live, shown.command);
        return true;
      }
      if (plain && event.key === 'Escape') {
        rejectAnswer();
        return true;
      }
      if (ctrl && name === 'e') {
        editing = shown.command;
        m.redraw();
        return true;
      }
      if (ctrl && name === 'c') {
        rejectAnswer();
        return true;
      }
    }

    if (live === undefined || !ctrlShift) return false;
    if (!assistantOn() && (name === 'e' || name === 'f')) return false;
    if (name === 'e') {
      explain(live);
      return true;
    }
    if (name === 'f') {
      fix(live);
      return true;
    }
    if (name === 's') return saveAsNote(live);
    const block = live.blocks.blocks.at(-1);
    if (block === undefined) return false;
    if (name === 'c') copyText(blockOutput(live, block));
    else if (name === 'r') rerunBlock(live, block);
    else return false;
    return true;
  }

  function rejectAnswer(): void {
    panel = undefined;
    editing = undefined;
    m.redraw();
  }

  /** Accepts the shown suggestion by typing its remainder, as if the person had typed it. */
  function acceptSuggestion(live: LiveTerminal): boolean {
    if (live.suggestion === undefined || live.sessionId === undefined) return false;
    const remainder = live.suggestion;
    live.typed = { text: `${live.typed.text}${remainder}`, known: true };
    live.suggestion = undefined;
    writeToShell(live, remainder);
    m.redraw();
    return true;
  }

  /**
   * Handles keyboard input the person types into the terminal. A line that starts with `#` and is
   * submitted is a question for the assistant: it is cleared from the prompt and never run.
   */
  /** Keyboard input: held until the shell is ready, then tracked and sent. */
  function typeInput(live: LiveTerminal, data: string): void {
    if (!live.ready) live.pendingInput.push(data);
    else if (live.sessionId !== undefined) onTypedData(live, data);
  }

  /** Most questions remembered per tab. */
  const MAX_QUESTIONS = 100;

  /**
   * Up/Down on a line that starts with `#` walks through the questions asked in this tab, like a
   * shell's history; Down past the newest brings back what was typed. Other lines keep the shell's
   * own history.
   */
  function recallQuestion(live: LiveTerminal, direction: 'older' | 'newer'): boolean {
    if (!assistantOn() || !live.ready || isBusy(live) || live.sessionId === undefined) return false;
    const recalling = live.recallIndex !== undefined;
    if (!recalling && !(live.typed.known && live.typed.text.startsWith('#'))) return false;
    const last = live.questions.length - 1;
    if (last < 0) return false;
    let index: number | undefined;
    let draft = live.recallDraft;
    if (direction === 'older') {
      if (live.recallIndex === undefined) draft = live.typed.text;
      index = live.recallIndex === undefined ? last : Math.max(0, live.recallIndex - 1);
    } else {
      if (live.recallIndex === undefined) return false;
      index = live.recallIndex >= last ? undefined : live.recallIndex + 1;
    }
    const text = index === undefined ? draft : `# ${live.questions[index] ?? ''}`;
    // Typed as input, so the shell shows it and the typed-line tracker follows. The text comes
    // from the tracker, which never holds control characters.
    onTypedData(live, `\x15${text}`);
    live.recallDraft = draft;
    live.recallIndex = index;
    return true;
  }

  /**
   * Tab on a `#` line: `# /par` completes a command or skill name; `@par` completes a path under
   * the session's root (task 0343), or in the terminal's folder when the host cannot list a tree.
   */
  function completeMentionInShell(live: LiveTerminal): boolean {
    const attrs = currentAttrs;
    const location = attrs?.location;
    if (attrs === undefined || !assistantOn() || !live.ready || isBusy(live)) return false;
    if (location === undefined) return false;
    const typed = live.typed;
    if (!typed.known || !typed.text.startsWith('#')) return false;
    const apply = (completion: { erase: number; insert: string } | undefined): void => {
      // The line may have changed while the names were read.
      if (completion === undefined || live.typed.text !== typed.text) return;
      typeInput(live, '\x7f'.repeat(completion.erase) + completion.insert);
    };
    const warn = (error: unknown): void => {
      console.warn('Reading names for completion failed.', error);
    };
    if (typedSlash(typed.text) !== undefined) {
      void skillNames(live, attrs.client, live.assistRoot ?? location)
        .then((skills) => apply(completeSlash(typed.text, slashNames(skills))))
        .catch(warn);
      return true;
    }
    if (!/(?:^|\s)@(?:"[^"]*|'[^']*|\S*)$/.test(typed.text)) return false;
    if (attrs.client.assistFiles !== undefined) {
      const root = live.assistRoot ?? location;
      void treeFiles(live, attrs.client, root)
        .then((files) => apply(completeMentionInTree(typed.text, files)))
        .catch(warn);
      return true;
    }
    if (attrs.listFolder === undefined) return false;
    void attrs
      .listFolder(location)
      .then((names) => apply(completeMention(typed.text, names)))
      .catch(warn);
    return true;
  }

  /** Marks the completion listings as possibly out of date. */
  function markStale(live: LiveTerminal): void {
    if (live.fileCache !== undefined) live.fileCache.stale = true;
    if (live.skillCache !== undefined) live.skillCache.stale = true;
  }

  /** The paths under `root`, from the cache unless something may have changed them. */
  async function treeFiles(
    live: LiveTerminal,
    client: TerminalClient,
    root: Location,
  ): Promise<readonly string[]> {
    const cached = live.fileCache;
    if (cached !== undefined && cached.root === root.uri && !cached.stale) return cached.names;
    const listed = (await client.assistFiles?.(root.uri)) ?? { files: [], truncated: false };
    live.fileCache = { root: root.uri, names: listed.files, stale: false };
    return listed.files;
  }

  /** The skill names `/` completes in `root`, cached like the paths. */
  async function skillNames(
    live: LiveTerminal,
    client: TerminalClient,
    root: Location,
  ): Promise<readonly string[]> {
    const cached = live.skillCache;
    if (cached !== undefined && cached.root === root.uri && !cached.stale) return cached.names;
    const skills = (await client.assistSkills?.(root.uri)) ?? [];
    const names = skills.map((skill) => skill.name);
    live.skillCache = { root: root.uri, names, stale: false };
    return names;
  }

  /** A `#` question: a `/` command runs here, `/skill rest` asks with that skill, else it is asked. */
  function submitQuestion(live: LiveTerminal, question: string): void {
    const slash = parseSlash(question);
    if (slash === undefined) {
      askAssistant(live, question);
      return;
    }
    if (slash.kind === 'skill') {
      const asked =
        slash.rest === '' ? t('terminal', 'slashUseSkill', { name: slash.name }) : slash.rest;
      askAssistant(live, asked, { display: question, skill: slash.name });
      return;
    }
    runSlashCommand(live, question, slash.name, slash.argument);
  }

  /** Writes a command's output under its question line, then gives the prompt back. */
  function showCommandOutput(live: LiveTerminal, question: string, text: string): void {
    live.terminal.write(`\r\x1b[2K${questionAnsi(question)}${text}`, () => releasePrompt(live));
  }

  /** Runs `/new`, `/clear`, `/compact`, `/model` or `/help` (task 0343). */
  function runSlashCommand(
    live: LiveTerminal,
    question: string,
    name: SlashCommandName,
    argument: string,
  ): void {
    const attrs = currentAttrs;
    if (attrs === undefined) return;
    live.typed = EMPTY_INPUT;
    live.suggestion = undefined;
    const forget = (): void => {
      live.requestId += 1;
      void attrs.client.assistClose(live.assistSession).catch(() => undefined);
      live.models = [];
    };
    switch (name) {
      case 'new': {
        forget();
        live.assistRoot = attrs.location;
        live.fileCache = undefined;
        live.skillCache = undefined;
        live.assistActive = false;
        const folder = attrs.location?.uri ?? '';
        showCommandOutput(live, question, infoAnsi([t('terminal', 'slashNew', { folder })]));
        return;
      }
      case 'clear':
        forget();
        showCommandOutput(live, question, infoAnsi([t('terminal', 'slashCleared')]));
        return;
      case 'help':
        showCommandOutput(
          live,
          question,
          infoAnsi(SLASH_COMMANDS.map((command) => `/${command}  ${slashHelp(command)}`)),
        );
        return;
      case 'compact':
        runAsyncCommand(live, question, async () => {
          if (attrs.client.assistCompact === undefined)
            return noticeAnsi(t('terminal', 'slashUnsupported'));
          await attrs.client.assistCompact(live.assistSession);
          return infoAnsi([t('terminal', 'slashCompacted')]);
        });
        return;
      case 'model':
        runAsyncCommand(live, question, () => switchModel(live, attrs.models, argument));
        return;
    }
  }

  /** Runs a command that waits on the host, showing it as busy meanwhile. */
  function runAsyncCommand(
    live: LiveTerminal,
    question: string,
    work: () => Promise<string>,
  ): void {
    live.requestId += 1;
    const token = live.requestId;
    panel = { status: 'busy', tabKey: live.key };
    live.terminal.write(`\r\x1b[2K${questionAnsi(question)}`);
    m.redraw();
    work()
      .then((text) => present(live, token, text, undefined))
      .catch((error: unknown) =>
        present(live, token, noticeAnsi(assistFailureText(String(error))), undefined),
      )
      .finally(() => m.redraw());
  }

  /** `/model`: lists the chat models; `/model <n|name>` switches the chat role to one. */
  async function switchModel(
    live: LiveTerminal,
    models: TerminalModels | undefined,
    argument: string,
  ): Promise<string> {
    if (models === undefined) return noticeAnsi(t('terminal', 'slashUnsupported'));
    if (argument !== '' && live.models.length > 0) {
      const choice = pickModel(live.models, argument);
      if (choice !== undefined) {
        await models.select(choice);
        live.models = live.models.map((known) => ({
          ...known,
          current: known.profileId === choice.profileId && known.model === choice.model,
        }));
        return infoAnsi([
          t('terminal', 'slashModelSet', { model: choice.model, profile: choice.profileName }),
        ]);
      }
    }
    const { profiles, chat } = await models.list();
    const discovered = new Map<string, readonly string[]>();
    await Promise.all(
      profiles.map(async (profile) => {
        const found = await models.discover(profile.id).catch(() => []);
        discovered.set(profile.id, found);
      }),
    );
    live.models = modelChoices(profiles, chat, discovered);
    if (argument !== '') {
      const choice = pickModel(live.models, argument);
      if (choice === undefined)
        return noticeAnsi(t('terminal', 'slashModelUnknown', { name: argument }));
      return switchModel(live, models, argument);
    }
    if (live.models.length === 0) return noticeAnsi(t('terminal', 'assistRoleNotAssigned'));
    return infoAnsi([...modelLines(live.models), t('terminal', 'slashModelPick')]);
  }

  function onTypedData(live: LiveTerminal, data: string): void {
    if (isBusy(live)) {
      // The terminal shows the answer where the prompt was; keys wait for the prompt to return.
      live.held.push(data);
      return;
    }
    live.recallIndex = undefined;
    const atPrompt = live.atPrompt || !live.markSeen;
    const question =
      data === '\r' && atPrompt && assistantOn()
        ? questionAtPrompt(live.typed, cursorLine(live.terminal))
        : undefined;
    if (question !== undefined) {
      live.typed = EMPTY_INPUT;
      live.suggestion = undefined;
      writeToShell(live, '\x15');
      if (question !== '') {
        if (live.questions.at(-1) !== question) live.questions.push(question);
        if (live.questions.length > MAX_QUESTIONS) live.questions.shift();
        submitQuestion(live, question);
      }
      m.redraw();
      return;
    }
    const wasComposing = live.typed.text.startsWith('#');
    const applied = applyTypedInput(live.typed, data);
    live.typed = applied.state;
    if (wasComposing !== live.typed.text.startsWith('#')) m.redraw();
    if (applied.submitted !== undefined && applied.submitted.trim() !== '') {
      const command = applied.submitted.trim();
      live.pendingCommand = command;
      // The command may create or remove files.
      markStale(live);
      live.history.push(command);
      if (live.history.length > MAX_HISTORY)
        live.history.splice(0, live.history.length - MAX_HISTORY);
    }
    const previous = live.suggestion;
    live.suggestion = atPrompt ? suggestionRemainder(live.history, live.typed) : undefined;
    writeToShell(live, data);
    if (previous !== live.suggestion) m.redraw();
  }

  function ensure(attrs: TerminalDrawerAttrs, element: HTMLElement): void {
    if (!attrs.open) return;
    const location = attrs.location;
    const key = attrs.tabKey;
    if (location === undefined || key === undefined) return;
    const host = terminalHost(location);
    let live = terminals.get(key);
    if (live !== undefined && live.host !== host) {
      // The tab moved to another machine (local <-> SSH, or another connection): the shell it
      // holds would keep running on the old one.
      disposeLive(live);
      terminals.delete(key);
      live = undefined;
    }
    if (live === undefined) {
      const inheritedFontSize = Number.parseFloat(getComputedStyle(element).fontSize);
      const style = getComputedStyle(element);
      // xterm.js otherwise defaults to "courier-new, courier, monospace", which renders
      // noticeably larger than the app's own text at the same nominal font size.
      const inheritedFontFamily = style.getPropertyValue('--fm-font-mono').trim();
      const terminal = new Terminal({
        // `registerDecoration` (the block icons) is proposed API in xterm.js and throws without it.
        allowProposedApi: true,
        cursorBlink: true,
        convertEol: true,
        // Lighter than xterm's 400/700, closer to the weight of the panes' proportional text.
        fontWeight: 300,
        fontWeightBold: 500,
        fontSize: Number.isFinite(inheritedFontSize) ? inheritedFontSize : 12.88,
        ...(inheritedFontFamily === '' ? {} : { fontFamily: inheritedFontFamily }),
        theme: {
          background: style.getPropertyValue('--fm-surface').trim(),
          foreground: style.getPropertyValue('--fm-text').trim(),
          cursor: style.getPropertyValue('--fm-accent').trim(),
          selectionBackground: style.getPropertyValue('--fm-selection').trim(),
          ...ansiPalette(style),
        },
      });
      const fit = new FitAddon();
      terminal.loadAddon(fit);
      const surface = document.createElement('div');
      surface.className = 'fm-terminal-surface';
      const decorations = new Map<number, IDecoration>();
      const blocks = createBlockTracker<IMarker>({
        maxBlocks: MAX_BLOCKS,
        onEvict: (block) => {
          decorations.get(block.id)?.dispose();
          decorations.delete(block.id);
          for (const mark of [block.promptMark, block.outputMark, block.endMark]) mark.dispose();
        },
      });
      live = {
        key,
        host,
        terminal,
        fit,
        surface,
        blocks,
        decorations,
        typed: EMPTY_INPUT,
        folder: undefined,
        history: [],
        questions: [],
        recallIndex: undefined,
        recallDraft: '',
        pendingCommand: '',
        atPrompt: true,
        markSeen: false,
        suggestion: undefined,
        assistSession: crypto.randomUUID(),
        // The other pane is rarely needed; the person can switch it on with its chip.
        removedChips: new Set<RemovableChipId>(['otherFolder']),
        awaiting: undefined,
        assistActive: false,
        held: [],
        requestId: 0,
        restoreLine: undefined,
        touchedChips: new Set(),
        ready: false,
        pendingInput: [],
        readyTimer: undefined,
        assistRoot: undefined,
        fileCache: undefined,
        skillCache: undefined,
        models: [],
      };
      terminals.set(key, live);
      const current = live;
      terminal.parser.registerOscHandler(133, (payload) => {
        onShellMark(current, payload);
        return true;
      });
      terminal.attachCustomKeyEventHandler((event) =>
        handleTerminalKeyEvent(event, {
          onToggle: () => {
            attrs.onToggle();
            m.redraw();
          },
          ...(attrs.onSwitchPane === undefined ? {} : { onSwitchPane: attrs.onSwitchPane }),
          ...(attrs.onCycleTab === undefined ? {} : { onCycleTab: attrs.onCycleTab }),
          ...(attrs.onFocusFolder === undefined ? {} : { onFocusFolder: attrs.onFocusFolder }),
          onAcceptSuggestion: () => acceptSuggestion(current),
          onAssistantKey: (event) => assistantKey(event),
          onCompleteMention: () => completeMentionInShell(current),
          onRecallQuestion: (direction) => recallQuestion(current, direction),
          onShellInput: (data) => typeInput(current, data),
        }),
      );
      terminal.onData((data) => typeInput(current, data));
      showTerminalSurface(element, surface);
      terminal.open(surface);
    } else {
      showTerminalSurface(element, live.surface);
    }
    live.fit.fit();
    paneFolderChanged(live, attrs);
    const session = live;
    if (session.sessionId === undefined && session.opening === undefined) {
      const token = {};
      session.openToken = token;
      session.ready = false;
      session.opening = attrs.client
        .open(
          location,
          session.terminal.cols,
          session.terminal.rows,
          (event) => {
            if (session.openToken !== token) return;
            if (event.type === 'output') {
              session.terminal.write(new Uint8Array(event.data));
              // Ready after the output goes quiet; a shell with marks says so earlier, and one
              // that should send marks gets longer before it is trusted without them.
              armReady(session, attrs.shellIntegration === 'enabled' ? 1500 : 250);
            } else if (event.type === 'exited') {
              session.terminal.write(`\r\n\x1b[90m[${t('terminal', 'sessionEnded')}]\x1b[0m\r\n`);
              // Clear the dead session id so the next toggle/reopen redials
              // instead of silently reusing a session the backend already
              // discarded (the backend removes it from its own registry too).
              session.sessionId = undefined;
            }
          },
          attrs.shellIntegration === 'enabled',
        )
        .then((id) => {
          session.opening = undefined;
          if (session.openToken !== token) {
            // The session was restarted while it opened: this one is no longer wanted.
            void attrs.client.close(id);
            return;
          }
          session.sessionId = id;
          if (session.readyTimer === undefined) armReady(session, 400);
        })
        .catch(() => {
          session.opening = undefined;
        });
    } else if (session.sessionId !== undefined) {
      void attrs.client.resize(session.sessionId, session.terminal.cols, session.terminal.rows);
    }
  }

  /** The answer panel and the suggestion, floated over the terminal's top-right corner. */
  /**
   * The strip under the terminal output: the consent prompt, the history suggestion and the
   * assistant bar. It only takes room while it has something to show. The conversation itself is
   * written into the terminal, so there is no second scrolling area (tasks 0333, 0336).
   */
  function stripView(attrs: TerminalDrawerAttrs): m.Children {
    if (attrs.location === undefined || attrs.tabKey === undefined) return undefined;
    const live = terminals.get(attrs.tabKey);
    const shown = panel !== undefined && panel.tabKey === attrs.tabKey ? panel : undefined;
    const consent = consentView(attrs, live);
    const suggestion =
      live?.suggestion === undefined
        ? undefined
        : m('.fm-terminal-suggestion', [
            m('span.fm-terminal-suggestion-hint', t('terminal', 'suggestionAccept')),
            m('code', highlightedShell(`${live.typed.text}${live.suggestion}`)),
          ]);
    const bar = live === undefined ? undefined : assistBar(live, shown);
    if (consent === undefined && suggestion === undefined && bar === undefined) return undefined;
    return m('.fm-terminal-strip', [suggestion, consent, bar]);
  }

  /** An action button with its keyboard shortcut shown beside the label. */
  function shortcutButton(label: string, shortcut: string | undefined, onclick: () => void) {
    return m(
      'button.fm-terminal-assist-action',
      {
        type: 'button',
        disabled: shortcut === undefined,
        'aria-keyshortcuts': shortcut,
        onclick,
      },
      [label, shortcut === undefined ? undefined : m('kbd.fm-terminal-kbd', shortcut)],
    );
  }

  /** The label of one context chip. */
  function chipLabel(chip: ContextChip): string {
    switch (chip.id) {
      case 'environment':
        return t('terminal', 'contextEnvironment');
      case 'listing':
        return t('terminal', 'contextListing', { count: String(chip.count ?? 0) });
      case 'selection':
        return t('terminal', 'contextSelection', { count: String(chip.count ?? 0) });
      case 'otherFolder':
        return t('terminal', 'contextOtherFolder');
      case 'blocks':
        return t('terminal', 'contextBlocks', { count: String(chip.count ?? 0) });
      case 'file':
        return t('terminal', 'contextFile', { name: chip.name ?? '' });
    }
  }

  /** What a chip contains, and what clicking it does. The browser shows it after a short delay. */
  function chipTooltip(chip: ContextChip): string {
    const contents =
      chip.id === 'environment' ? t('terminal', 'contextEnvironmentDetail') : (chip.detail ?? '');
    if (chip.id === 'file') return contents;
    const action = chip.removed ? t('terminal', 'contextRestore') : t('terminal', 'contextRemove');
    return contents === '' ? action : `${contents}\n\n${action}`;
  }

  /**
   * What would be sent with the next question, one chip per part. A chip toggles its part off and
   * on; `@path` in the question adds that file. On a narrow bar the pills fold into one button.
   */
  function contextRow(live: LiveTerminal, questionText: string): m.Children {
    const source = assistSource(live);
    const removed = removedFor(live, source);
    const chips = contextChips(source, removed, parseMentions(questionText));
    const included = chips.filter((chip) => !chip.removed).length;
    const pills = m(
      '.fm-terminal-context',
      {
        class: contextOpen ? 'fm-terminal-context--open' : undefined,
        role: 'group',
        'aria-label': t('terminal', 'contextLabel'),
      },
      chips.map((chip) =>
        m(
          'button.fm-terminal-chip',
          {
            type: 'button',
            class: chip.removed ? 'fm-terminal-chip--off' : undefined,
            disabled: chip.id === 'file',
            'aria-pressed': chip.id === 'file' ? undefined : String(!chip.removed),
            'aria-label': `${chipLabel(chip)}. ${chipTooltip(chip)}`,
            ...tooltipEvents(() => chipTooltip(chip), 'above-left'),
            onclick: () => {
              if (chip.id === 'file') return;
              live.touchedChips.add(chip.id);
              if (removed.has(chip.id)) live.removedChips.delete(chip.id);
              else live.removedChips.add(chip.id);
              m.redraw();
            },
          },
          chipLabel(chip),
        ),
      ),
    );
    return m('.fm-terminal-context-wrap', [
      m(
        'button.fm-terminal-context-toggle',
        {
          type: 'button',
          'aria-expanded': String(contextOpen),
          onclick: () => {
            contextOpen = !contextOpen;
            m.redraw();
          },
        },
        t('terminal', 'contextToggle', { count: String(included) }),
      ),
      pills,
    ]);
  }

  /** The help dialog for the terminal assistant: what to type, what is sent, what runs. */
  function helpDialog(): m.Children {
    const close = () => {
      helpOpen = false;
      currentAttrs?.tabKey === undefined
        ? undefined
        : terminals.get(currentAttrs.tabKey)?.terminal.focus();
      m.redraw();
    };
    const items = [
      'helpAsk',
      'helpContext',
      'helpMentions',
      'helpExplainFix',
      'helpFollowUp',
      'helpRun',
      'helpSafety',
    ] as const;
    return m(
      '.fm-terminal-help-backdrop',
      {
        onclick: (event: MouseEvent) => {
          if (event.target === event.currentTarget) close();
        },
      },
      m(
        '.fm-terminal-help-dialog',
        {
          role: 'dialog',
          'aria-modal': 'true',
          'aria-labelledby': 'fm-terminal-help-title',
          tabindex: -1,
          oncreate: ({ dom }: m.VnodeDOM) => (dom as HTMLElement).focus(),
          onkeydown: (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
              event.preventDefault();
              event.stopPropagation();
              close();
            }
          },
        },
        [
          m('h2#fm-terminal-help-title', t('terminal', 'helpTitle')),
          m(
            'ul',
            items.map((item) => m('li', t('terminal', item))),
          ),
          m(
            'button.fm-terminal-help-close',
            { type: 'button', onclick: close },
            t('terminal', 'helpClose'),
          ),
        ],
      ),
    );
  }

  /** The command being edited with Ctrl+E, as a text field. */
  function editField(live: LiveTerminal, value: string): m.Children {
    return m('input.fm-terminal-assist-edit', {
      type: 'text',
      value,
      'aria-label': t('terminal', 'assistInsert'),
      oncreate: ({ dom }: m.VnodeDOM) => (dom as HTMLInputElement).focus(),
      oninput: (event: Event) => {
        editing = (event.target as HTMLInputElement).value;
      },
      onkeydown: (event: KeyboardEvent) => {
        if (event.key === 'Enter' && editing !== undefined) {
          event.preventDefault();
          insertCommand(live, editing);
        } else if (event.key === 'Escape') {
          event.preventDefault();
          editing = undefined;
          live.terminal.focus();
          m.redraw();
        }
      },
    });
  }

  /** "Thinking" with one, two, then three dots, animated by CSS; the dots keep their room. */
  function thinkingStatus(): m.Children {
    return m('span.fm-terminal-assist-status.fm-terminal-assist-thinking', { role: 'status' }, [
      t('terminal', 'assistBusy').replace(/[….]+$/, ''),
      m('span.fm-dots', { 'aria-hidden': 'true' }, [
        m('span', '.'),
        m('span', '.'),
        m('span', '.'),
      ]),
    ]);
  }

  /** What the assistant waits on: the answer, a command to insert, approve, or its progress. */
  function statusRow(shown: AssistPanel, live: LiveTerminal): m.Children {
    const command = (text: string) => m('code.fm-terminal-assist-command', highlightedShell(text));
    switch (shown.status) {
      case 'busy':
        // Shown in the controls row instead, so the strip does not grow while the model thinks.
        return undefined;
      case 'suggest':
        return m('.fm-terminal-assist-row', [
          editing === undefined ? command(shown.command) : editField(live, editing),
          m('.fm-terminal-assist-buttons', [
            shortcutButton(t('terminal', 'assistInsert'), 'Enter', () =>
              insertCommand(live, shown.command),
            ),
            shortcutButton(t('terminal', 'assistEdit'), 'Ctrl+E', () => {
              editing = shown.command;
              m.redraw();
            }),
            shortcutButton(t('terminal', 'assistDismiss'), 'Esc', () => rejectAnswer()),
          ]),
        ]);
      case 'approve': {
        const canRun = live.markSeen;
        return m('.fm-terminal-assist-row', [
          command(shown.command),
          // The command and its Run / Decline say what is asked; only the exception needs words.
          canRun
            ? undefined
            : m('span.fm-terminal-assist-status', t('terminal', 'assistNeedsBlocks')),
          m('.fm-terminal-assist-buttons', [
            shortcutButton(
              canRun ? t('terminal', 'assistRun') : t('terminal', 'assistInsert'),
              'Enter',
              () => {
                if (canRun) runApproved(live, shown.command);
                else insertCommand(live, shown.command);
              },
            ),
            shortcutButton(t('terminal', 'assistDecline'), 'Ctrl+C', () =>
              declineApproved(live, shown.command),
            ),
          ]),
        ]);
      }
      case 'write':
        return m('.fm-terminal-assist-row', [
          m('span.fm-terminal-assist-status', [
            t('terminal', 'assistWriteAsk', { folder: shown.folder }),
            ' ',
            m('code.fm-terminal-assist-command', `${shown.name} (${byteSize(shown.bytes)})`),
          ]),
          m('.fm-terminal-assist-buttons', [
            shortcutButton(t('terminal', 'assistWriteAllow'), 'Enter', () =>
              answerWrite(live, 'question'),
            ),
            m(
              'button.fm-terminal-assist-action',
              { type: 'button', onclick: () => answerWrite(live, 'session') },
              t('terminal', 'assistWriteSession'),
            ),
            shortcutButton(t('terminal', 'assistDecline'), 'Ctrl+C', () =>
              answerWrite(live, 'decline'),
            ),
          ]),
        ]);
      case 'running':
        return m('.fm-terminal-assist-row', [
          command(shown.command),
          m('span.fm-terminal-assist-status', t('terminal', 'assistRunning')),
          m('.fm-terminal-assist-buttons', [
            shortcutButton(t('terminal', 'assistStop'), 'Esc', () => {
              live.awaiting = undefined;
              rejectAnswer();
            }),
          ]),
        ]);
    }
  }

  /** Explain is offered once there is output to explain (or when blocks are not tracked). */
  function offerExplain(live: LiveTerminal): boolean {
    return !live.markSeen || live.blocks.blocks.length > 0;
  }

  /** Fix is offered after a failed command (or when blocks are not tracked, so failure is unknown). */
  function offerFix(live: LiveTerminal): boolean {
    if (!live.markSeen) return true;
    const exitCode = live.blocks.blocks.at(-1)?.exitCode;
    return exitCode !== undefined && exitCode !== 0;
  }

  /**
   * The bar under the terminal while the assistant is in use: what it waits on, the context pills
   * and Explain / Fix. It shows while a `#` question is typed, so the pills can be trimmed first.
   */
  function assistBar(live: LiveTerminal, shown: AssistPanel | undefined): m.Children {
    if (!assistantOn()) return undefined;
    const typing = live.typed.text.startsWith('#');
    if (!live.assistActive && !typing && shown === undefined) return undefined;
    const busy = shown?.status === 'busy';
    return m('.fm-terminal-assist-bar', [
      shown === undefined ? undefined : statusRow(shown, live),
      m('.fm-terminal-assist-controls', [
        contextRow(live, typing ? live.typed.text : ''),
        m('.fm-terminal-assist-buttons', [
          busy ? thinkingStatus() : undefined,
          busy
            ? shortcutButton(t('terminal', 'assistDismiss'), 'Esc', () => cancelBusy(live))
            : undefined,
          busy || !offerExplain(live)
            ? undefined
            : shortcutButton(t('terminal', 'blockExplain'), chordLabel('E'), () => explain(live)),
          busy || !offerFix(live)
            ? undefined
            : shortcutButton(t('terminal', 'blockFix'), chordLabel('F'), () => fix(live)),
          busy || currentAttrs?.onSaveAsNote === undefined
            ? undefined
            : m(
                'button.fm-terminal-assist-action',
                {
                  type: 'button',
                  'aria-keyshortcuts': chordLabel('S'),
                  ...tooltipEvents(() => t('notes', 'saveAsNoteHelp'), 'above-left'),
                  onclick: () => saveAsNote(live),
                },
                [t('notes', 'saveAsNote'), m('kbd.fm-terminal-kbd', chordLabel('S'))],
              ),
          live.assistActive && shown === undefined
            ? m(
                'button.fm-terminal-assist-action',
                {
                  type: 'button',
                  'aria-label': t('terminal', 'assistDismiss'),
                  onclick: () => {
                    live.assistActive = false;
                    m.redraw();
                  },
                },
                '×',
              )
            : undefined,
        ]),
      ]),
    ]);
  }

  /** Ends the tab's shell so the next open starts it with the current integration setting. */
  function restartSession(live: LiveTerminal): void {
    const id = live.sessionId;
    live.sessionId = undefined;
    live.openToken = undefined;
    live.ready = false;
    live.suggestion = undefined;
    live.typed = EMPTY_INPUT;
    live.terminal.reset();
    if (id !== undefined && currentAttrs !== undefined) void currentAttrs.client.close(id);
    m.redraw();
  }

  /** Records the person's answer to the consent prompt; enabling restarts this tab's shell. */
  function chooseShellIntegration(live: LiveTerminal | undefined, choice: 'enabled' | 'disabled') {
    const save = currentAttrs?.onShellIntegrationChoice;
    if (save === undefined) return;
    save(choice).then(
      () => {
        if (choice === 'enabled' && live !== undefined) restartSession(live);
      },
      (error: unknown) => {
        console.error('Saving the command-block choice failed.', error);
      },
    );
  }

  /** Asked once, until answered: explains what changes and lets the person decide. */
  function consentView(attrs: TerminalDrawerAttrs, live: LiveTerminal | undefined): m.Children {
    if (attrs.shellIntegration !== 'unset' || attrs.onShellIntegrationChoice === undefined) {
      return undefined;
    }
    return m(
      '.fm-terminal-consent',
      { role: 'region', 'aria-label': t('terminal', 'consentTitle') },
      [
        m('p.fm-terminal-consent-text', t('terminal', 'consentText')),
        m('.fm-terminal-assist-buttons', [
          shortcutButton(t('terminal', 'consentEnable'), 'Alt+Y', () =>
            chooseShellIntegration(live, 'enabled'),
          ),
          shortcutButton(t('terminal', 'consentDecline'), 'Alt+N', () =>
            chooseShellIntegration(live, 'disabled'),
          ),
        ]),
      ],
    );
  }

  return {
    oninit: ({ attrs }) => {
      currentAttrs = attrs;
      attrs.registerFocus?.(() => {
        const key = currentAttrs?.tabKey;
        const live = key === undefined ? undefined : terminals.get(key);
        if (currentAttrs?.open !== true || live === undefined) return false;
        live.terminal.focus();
        return true;
      });
      attrs.registerInsert?.((text, host) => insertAtPrompt(text, host));
      attrs.registerSaveAsNote?.(() => {
        const key = currentAttrs?.tabKey;
        return saveAsNote(key === undefined ? undefined : terminals.get(key));
      });
      attrs.registerDisposeTab?.((key) => {
        const live = terminals.get(key);
        if (live === undefined) return;
        disposeLive(live);
        terminals.delete(key);
      });
    },
    onremove: () => {
      observer?.disconnect();
      for (const live of terminals.values()) disposeLive(live);
    },
    view: ({ attrs }) => {
      currentAttrs = attrs;
      return m(
        '.fm-terminal-drawer',
        { hidden: !attrs.open, 'aria-label': t('terminal', 'drawer') },
        [
          m('.fm-terminal-resize-handle', {
            onpointerdown: (event: PointerEvent) => {
              const drawer = (event.currentTarget as HTMLElement).parentElement;
              if (drawer === null) return;
              const startY = event.clientY;
              const startHeight = drawer.getBoundingClientRect().height;
              const move = (moveEvent: PointerEvent) => {
                const height = Math.max(
                  120,
                  Math.min(window.innerHeight * 0.8, startHeight + startY - moveEvent.clientY),
                );
                drawer.style.height = `${height}px`;
                attrs.onResize?.(height);
                const live = attrs.tabKey === undefined ? undefined : terminals.get(attrs.tabKey);
                live?.fit.fit();
                if (live?.sessionId !== undefined)
                  void attrs.client.resize(live.sessionId, live.terminal.cols, live.terminal.rows);
              };
              const up = () => {
                window.removeEventListener('pointermove', move);
                window.removeEventListener('pointerup', up);
              };
              window.addEventListener('pointermove', move);
              window.addEventListener('pointerup', up);
            },
          }),
          m(
            'button.fm-terminal-close',
            {
              type: 'button',
              title: t('terminal', 'close'),
              'aria-label': t('terminal', 'close'),
              onclick: () => attrs.onToggle(),
            },
            '×',
          ),
          assistantOn()
            ? m(
                'button.fm-terminal-help',
                {
                  type: 'button',
                  'aria-label': t('terminal', 'help'),
                  ...tooltipEvents(() => t('terminal', 'help'), 'below-right'),
                  'aria-haspopup': 'dialog',
                  onclick: () => {
                    helpOpen = true;
                  },
                },
                helpIcon({ size: 14 }),
              )
            : undefined,
          helpOpen && assistantOn() ? helpDialog() : undefined,
          tip === undefined
            ? undefined
            : m(
                '.fm-terminal-tooltip',
                {
                  role: 'tooltip',
                  class: `fm-terminal-tooltip--${tip.place}`,
                  style: { left: `${tip.x}px`, top: `${tip.y}px` },
                },
                tip.text,
              ),
          attrs.location === undefined
            ? m('.fm-terminal-unavailable', t('terminal', 'selectDirectory'))
            : m('.fm-terminal-host', {
                oncreate: ({ dom }) => {
                  ensure(attrs, dom as HTMLElement);
                  if (typeof ResizeObserver !== 'undefined') {
                    observer = new ResizeObserver(() => ensure(attrs, dom as HTMLElement));
                    observer.observe(dom as HTMLElement);
                  }
                },
                onupdate: ({ dom }) => ensure(attrs, dom as HTMLElement),
              }),
          // Below the output, so the terminal shrinks while the strip has content.
          stripView(attrs),
        ],
      );
    },
  };
};
