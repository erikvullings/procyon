import '@xterm/xterm/css/xterm.css';

import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { Location } from '../../models';
import type { TerminalClient } from './terminal-client';

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
}

type LiveTerminal = {
  terminal: Terminal;
  fit: FitAddon;
  surface: HTMLElement;
  sessionId?: string | undefined;
  opening?: Promise<void> | undefined;
};

export interface TerminalKeyHandlers {
  readonly onToggle: () => void;
  readonly onSwitchPane?: () => void;
  readonly onCycleTab?: (direction: 1 | -1) => void;
  readonly onFocusFolder?: () => void;
  readonly onInput?: (data: string) => void;
  readonly usesApplicationCursorKeys?: () => boolean;
}

function webkitEditingKey(
  event: KeyboardEvent,
  applicationCursorKeys: boolean,
): string | undefined {
  if (event.keyCode !== 0 || event.altKey || event.ctrlKey || event.metaKey) return undefined;
  const cursorPrefix = applicationCursorKeys ? '\x1bO' : '\x1b[';
  switch (event.key) {
    case 'ArrowUp':
      return `${cursorPrefix}A`;
    case 'ArrowDown':
      return `${cursorPrefix}B`;
    case 'ArrowRight':
      return `${cursorPrefix}C`;
    case 'ArrowLeft':
      return `${cursorPrefix}D`;
    case 'Backspace':
      return '\x7f';
    case 'Home':
      return '\x1b[H';
    case 'End':
      return '\x1b[F';
    case 'Delete':
      return '\x1b[3~';
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
  const tabKey = event.key === 'Tab' || event.code === 'Tab';
  const navTabKey = tabKey && !event.metaKey && (event.ctrlKey || event.altKey);
  const handled = toggleKey || navTabKey;
  const editingInput =
    event.type === 'keydown'
      ? webkitEditingKey(event, handlers.usesApplicationCursorKeys?.() === true)
      : undefined;
  if (!handled && editingInput === undefined) return true;
  event.preventDefault();
  event.stopPropagation();
  if (toggleKey) handlers.onToggle();
  else if (navTabKey) {
    if (event.ctrlKey) handlers.onCycleTab?.(event.shiftKey ? -1 : 1);
    else if (event.shiftKey) handlers.onFocusFolder?.();
    else handlers.onSwitchPane?.();
  } else handlers.onInput?.(editingInput ?? '');
  return false;
}

/** Shows one location-owned terminal surface without confusing it with the shared drawer host. */
export function showTerminalSurface(host: HTMLElement, surface: HTMLElement): void {
  if (host.childElementCount !== 1 || host.firstElementChild !== surface) {
    host.replaceChildren(surface);
  }
}

/** A bottom drawer whose xterm instances remain mounted and keyed by backing location. */
export const TerminalDrawer: FactoryComponent<TerminalDrawerAttrs> = () => {
  const terminals = new Map<string, LiveTerminal>();
  let observer: ResizeObserver | undefined;
  let currentAttrs: TerminalDrawerAttrs | undefined;

  function ensure(attrs: TerminalDrawerAttrs, element: HTMLElement): void {
    if (!attrs.open) return;
    const location = attrs.location;
    const key = attrs.tabKey;
    if (location === undefined || key === undefined) return;
    let live = terminals.get(key);
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
        },
      });
      const fit = new FitAddon();
      terminal.loadAddon(fit);
      const surface = document.createElement('div');
      surface.className = 'fm-terminal-surface';
      live = { terminal, fit, surface };
      terminals.set(key, live);
      terminal.attachCustomKeyEventHandler((event) =>
        handleTerminalKeyEvent(event, {
          onToggle: () => {
            attrs.onToggle();
            m.redraw();
          },
          ...(attrs.onSwitchPane === undefined ? {} : { onSwitchPane: attrs.onSwitchPane }),
          ...(attrs.onCycleTab === undefined ? {} : { onCycleTab: attrs.onCycleTab }),
          ...(attrs.onFocusFolder === undefined ? {} : { onFocusFolder: attrs.onFocusFolder }),
          usesApplicationCursorKeys: () => terminal.modes.applicationCursorKeysMode,
          onInput: (data) => terminal.input(data, true),
        }),
      );
      terminal.onData((data) => {
        if (live?.sessionId !== undefined)
          void attrs.client.write(live.sessionId, new TextEncoder().encode(data));
      });
      showTerminalSurface(element, surface);
      terminal.open(surface);
    } else {
      showTerminalSurface(element, live.surface);
    }
    live.fit.fit();
    if (live.sessionId === undefined && live.opening === undefined) {
      live.opening = attrs.client
        .open(location, live.terminal.cols, live.terminal.rows, (event) => {
          if (event.type === 'output') live?.terminal.write(new Uint8Array(event.data));
          else if (event.type === 'exited') {
            live?.terminal.write(`\r\n\x1b[90m[${t('terminal', 'sessionEnded')}]\x1b[0m\r\n`);
            // Clear the dead session id so the next toggle/reopen redials
            // instead of silently reusing a session the backend already
            // discarded (the backend removes it from its own registry too).
            if (live !== undefined) live.sessionId = undefined;
          }
        })
        .then((id) => {
          if (live !== undefined) {
            live.sessionId = id;
            live.opening = undefined;
          }
        })
        .catch(() => {
          if (live !== undefined) live.opening = undefined;
        });
    } else if (live.sessionId !== undefined) {
      void attrs.client.resize(live.sessionId, live.terminal.cols, live.terminal.rows);
    }
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
        ],
      );
    },
  };
};
