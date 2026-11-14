import '@xterm/xterm/css/xterm.css';

import { FitAddon } from '@xterm/addon-fit';
import { type IDecoration, type IMarker, Terminal } from '@xterm/xterm';
import m, { type FactoryComponent } from 'mithril';
import { copyIcon, refreshIcon, sparklesIcon, wrenchIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { Location } from '../../models';
import type { TerminalShellIntegration } from '../../models/settings';
import { type BlockTracker, type CommandBlock, createBlockTracker } from './block-tracker';
import { highlightShell } from './shell-highlight';
import { parseShellMark } from './shell-integration';
import {
  cleanCommand,
  commandFromAnswer,
  type TerminalAssistKind,
  transcriptTail,
} from './terminal-assist';
import type { TerminalClient } from './terminal-client';
import { applyTypedInput, EMPTY_INPUT, suggestionRemainder, type TypedInput } from './typed-input';

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
  /** The person's choice for command-block shell integration (task 0333). */
  readonly shellIntegration?: TerminalShellIntegration;
  /** Hands the owner a way to type text at this drawer's active prompt (task 0333). */
  readonly registerInsert?: (insert: (text: string, host: string) => boolean) => void;
  /** Saves the choice; `undefined` hides the consent prompt. */
  readonly onShellIntegrationChoice?: (choice: 'enabled' | 'disabled') => Promise<void>;
}

/**
 * The machine a location's shell runs on: `local`, or `sftp://<connection-id>` for one SSH
 * connection. A tab keeps its terminal across folder navigation, but not across hosts.
 */
export function terminalHost(location: Location): string {
  const match = /^sftp:\/\/[^/]+/.exec(location.uri);
  return match?.[0] ?? 'local';
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
  /** Commands run in this session, oldest first, for suggestions. */
  history: string[];
  /** The command the person just ran; becomes the next block's command. */
  pendingCommand: string;
  /** Whether the shell is waiting for input (true before any shell mark is seen). */
  atPrompt: boolean;
  /** Whether the shell reports command marks; without them the terminal shows setup help. */
  markSeen: boolean;
  /** The rest of the suggested command, when one matches what is typed. */
  suggestion: string | undefined;
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

/** A finished block as text for the assistant: its command, then its output. */
function blockTranscript(live: LiveTerminal, block: CommandBlock<IMarker>): string {
  const head = `$ ${block.command}`;
  const { outputMark, endMark } = block;
  if (outputMark.line < 0 || endMark.line < outputMark.line) return head;
  return transcriptTail([head, ...linesText(live.terminal, outputMark.line, endMark.line)]);
}

/** The text of the terminal's cursor line (the line being edited), without its prompt-free trailing blanks. */
function cursorLine(terminal: Terminal): string {
  const buffer = terminal.buffer.active;
  return buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(true) ?? '';
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

/** An answer's text; the command of a `Command:` line is highlighted as shell. */
function answerNodes(kind: TerminalAssistKind, text: string): m.Children {
  if (kind === 'command') return highlightedShell(text);
  return text.split('\n').map((line) => {
    const match = /^(\s*Command:\s*)(.+)$/i.exec(line);
    return match
      ? [match[1], m('span.fm-shell-command-line', highlightedShell(match[2] ?? ''))]
      : line;
  });
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

/** The assistant's state for one tab: busy, an answer, or a failure. Shown only on that tab. */
type AssistPanel =
  | { readonly status: 'busy'; readonly tabKey: string; readonly kind: TerminalAssistKind }
  | {
      readonly status: 'done';
      readonly tabKey: string;
      readonly kind: TerminalAssistKind;
      readonly text: string;
      readonly command: string | undefined;
    }
  | {
      readonly status: 'failed';
      readonly tabKey: string;
      readonly kind: TerminalAssistKind;
      readonly reason: string;
    };

/** A bottom drawer whose xterm instances remain mounted and keyed by backing location. */
export const TerminalDrawer: FactoryComponent<TerminalDrawerAttrs> = () => {
  const terminals = new Map<string, LiveTerminal>();
  let observer: ResizeObserver | undefined;
  let currentAttrs: TerminalDrawerAttrs | undefined;
  let panel: AssistPanel | undefined;
  /** The command being edited with Ctrl+E before it is inserted; `undefined` when not editing. */
  let editing: string | undefined;

  /** Asks the chat endpoint about text from one terminal. Nothing is run; the answer is shown. */
  function runAssist(
    live: LiveTerminal,
    kind: TerminalAssistKind,
    transcript: string,
    request?: string,
  ): void {
    const attrs = currentAttrs;
    if (attrs?.location === undefined) return;
    if (kind !== 'command' && transcript.trim() === '') {
      panel = { status: 'failed', tabKey: live.key, kind, reason: 'noOutput' };
      m.redraw();
      return;
    }
    panel = { status: 'busy', tabKey: live.key, kind };
    m.redraw();
    attrs.client
      .assist({
        kind,
        transcript,
        ...(request === undefined ? {} : { request }),
        workingDirectory: attrs.location.uri,
      })
      .then(({ text }) => {
        const command = commandFromAnswer(kind, text);
        panel = {
          status: 'done',
          tabKey: live.key,
          kind,
          text,
          command: command === undefined ? undefined : cleanCommand(command),
        };
      })
      .catch((error: unknown) => {
        panel = { status: 'failed', tabKey: live.key, kind, reason: String(error) };
      })
      .finally(() => m.redraw());
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
  function onShellMark(live: LiveTerminal, payload: string): void {
    const mark = parseShellMark(payload);
    if (mark === undefined) return;
    live.markSeen = true;
    if (mark.kind === 'prompt') live.atPrompt = true;
    if (mark.kind === 'output') live.atPrompt = false;
    const block = live.blocks.apply(
      mark,
      () => live.terminal.registerMarker(0),
      () => live.pendingCommand,
    );
    if (block === undefined) return;
    live.atPrompt = true;
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
      action(t('terminal', 'blockExplain'), 'Ctrl+Shift+E', sparklesIcon({ size: 14 }), () =>
        runAssist(live, 'explain', blockTranscript(live, block)),
      ),
      failed
        ? action(t('terminal', 'blockFix'), 'Ctrl+Shift+F', wrenchIcon({ size: 14 }), () =>
            runAssist(live, 'fix', blockTranscript(live, block)),
          )
        : undefined,
      action(t('terminal', 'blockCopy'), 'Ctrl+Shift+C', copyIcon({ size: 14 }), () =>
        copyText(blockOutput(live, block)),
      ),
      action(t('terminal', 'blockRerun'), 'Ctrl+Shift+R', refreshIcon({ size: 14 }), () =>
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
   * again once the answer is gone. Ctrl+Shift+E/F/C/R act on the last command block. Alt+Y and
   * Alt+N answer the shell-integration prompt. Returns whether the key was used.
   */
  function assistantKey(event: KeyboardEvent): boolean {
    const attrs = currentAttrs;
    const key = attrs?.tabKey;
    const live = key === undefined ? undefined : terminals.get(key);
    const name = event.key.toLowerCase();
    const plain = !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    const ctrl = event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    const ctrlShift = event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey;

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
    if (shown?.status === 'done' && live !== undefined) {
      if (plain && event.key === 'Enter' && shown.command !== undefined) {
        insertCommand(live, shown.command);
        return true;
      }
      if (plain && event.key === 'Escape') {
        rejectAnswer();
        return true;
      }
      if (ctrl && name === 'e' && shown.command !== undefined) {
        editing = shown.command;
        m.redraw();
        return true;
      }
      if (ctrl && name === 'c') {
        rejectAnswer();
        return true;
      }
    }

    const block = live?.blocks.blocks.at(-1);
    if (live === undefined || block === undefined || !ctrlShift) return false;
    if (name === 'e') runAssist(live, 'explain', blockTranscript(live, block));
    else if (name === 'f' && block.exitCode !== 0)
      runAssist(live, 'fix', blockTranscript(live, block));
    else if (name === 'c') copyText(blockOutput(live, block));
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
  function onTypedData(live: LiveTerminal, data: string): void {
    const atPrompt = live.atPrompt || !live.markSeen;
    const question =
      data === '\r' && atPrompt
        ? questionAtPrompt(live.typed, cursorLine(live.terminal))
        : undefined;
    if (question !== undefined) {
      live.typed = EMPTY_INPUT;
      live.suggestion = undefined;
      writeToShell(live, '\x15');
      if (question !== '') runAssist(live, 'command', '', question);
      m.redraw();
      return;
    }
    const applied = applyTypedInput(live.typed, data);
    live.typed = applied.state;
    if (applied.submitted !== undefined && applied.submitted.trim() !== '') {
      const command = applied.submitted.trim();
      live.pendingCommand = command;
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
      live.terminal.dispose();
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
        cursorBlink: true,
        convertEol: true,
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
        history: [],
        pendingCommand: '',
        atPrompt: true,
        markSeen: false,
        suggestion: undefined,
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
        }),
      );
      terminal.onData((data) => {
        if (current.sessionId !== undefined) onTypedData(current, data);
      });
      showTerminalSurface(element, surface);
      terminal.open(surface);
    } else {
      showTerminalSurface(element, live.surface);
    }
    live.fit.fit();
    const session = live;
    if (session.sessionId === undefined && session.opening === undefined) {
      const token = {};
      session.openToken = token;
      session.opening = attrs.client
        .open(
          location,
          session.terminal.cols,
          session.terminal.rows,
          (event) => {
            if (session.openToken !== token) return;
            if (event.type === 'output') session.terminal.write(new Uint8Array(event.data));
            else if (event.type === 'exited') {
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
   * The assistant strip under the terminal output: the consent prompt, the suggestion and the
   * answer. It only takes room while it has something to show (Warp-style bottom strip, task 0333).
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
    if (consent === undefined && suggestion === undefined && shown === undefined) return undefined;
    return m('.fm-terminal-strip', [
      suggestion,
      consent,
      shown === undefined ? undefined : answerPanel(shown, live),
    ]);
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

  function answerPanel(shown: AssistPanel, live: LiveTerminal | undefined): m.Children {
    const busy = shown.status === 'busy';
    if (shown.status !== 'done') editing = undefined;
    const answer =
      shown.status === 'busy'
        ? m('p.fm-terminal-assist-status', t('terminal', 'assistBusy'))
        : shown.status === 'failed'
          ? m('p.fm-terminal-assist-status', assistFailureText(shown.reason))
          : editing !== undefined && live !== undefined
            ? m('input.fm-terminal-assist-edit', {
                type: 'text',
                value: editing,
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
                    m.redraw();
                  }
                },
              })
            : m('pre.fm-terminal-assist-text', answerNodes(shown.kind, shown.text));
    // The body scrolls; the buttons stay in a footer that is always in reach.
    return m('.fm-terminal-assist-panel', { role: 'status' }, [
      m('.fm-terminal-assist-body', [
        answer,
        shown.status === 'done' && shown.command === undefined
          ? m('p.fm-terminal-assist-status', t('terminal', 'assistNoCommand'))
          : undefined,
        m('p.fm-terminal-assist-note', t('terminal', 'assistSendsNote')),
      ]),
      m('.fm-terminal-assist-buttons', [
        shown.status === 'done' && shown.command !== undefined && live !== undefined
          ? shortcutButton(t('terminal', 'assistInsert'), 'Enter', () => {
              if (shown.status === 'done' && shown.command !== undefined) {
                insertCommand(live, shown.command);
              }
            })
          : undefined,
        shown.status === 'done' && shown.command !== undefined
          ? shortcutButton(t('terminal', 'assistEdit'), 'Ctrl+E', () => {
              editing = shown.command;
              m.redraw();
            })
          : undefined,
        shortcutButton(t('terminal', 'assistDismiss'), busy ? undefined : 'Ctrl+C', () =>
          rejectAnswer(),
        ),
      ]),
    ]);
  }

  /** Ends the tab's shell so the next open starts it with the current integration setting. */
  function restartSession(live: LiveTerminal): void {
    const id = live.sessionId;
    live.sessionId = undefined;
    live.openToken = undefined;
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
      attrs.registerDisposeTab?.((key) => {
        const live = terminals.get(key);
        if (live === undefined) return;
        live.terminal.dispose();
        terminals.delete(key);
      });
    },
    onremove: () => {
      observer?.disconnect();
      for (const live of terminals.values()) live.terminal.dispose();
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
