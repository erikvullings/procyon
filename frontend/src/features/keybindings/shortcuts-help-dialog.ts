import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import {
  getLiveBindings,
  type KeybindingRuntime,
  normalizedShortcut,
} from '../../keybindings/dispatcher';
import type { ActionDescriptor } from '../../models';
import type { SelectionPlatform } from '../selection/keybindings';

export interface ShortcutsHelpDialogAttrs {
  readonly open: boolean;
  readonly actions: readonly ActionDescriptor[];
  readonly keybindings: Readonly<Record<string, string>>;
  readonly platform: SelectionPlatform;
  readonly runtime: KeybindingRuntime;
  readonly onClose: () => void;
}

/** Modifier-only keydowns that must not be treated as a completed shortcut on their own. */
const BARE_MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta']);

/**
 * Read-only F1 "keyboard shortcuts" overlay (Total Commander parity, task 0128). Reuses
 * `getLiveBindings` - the same live-binding resolution the settings editor's conflict-detection
 * view is built on - rather than a second hardcoded shortcut list that could drift from the
 * actual registry. Also supports a text filter (name/description/shortcut) and a "press a key" capture
 * field that looks up whichever action, if any, is bound to the pressed combination.
 */
export const ShortcutsHelpDialog: FactoryComponent<ShortcutsHelpDialogAttrs> = () => {
  let query = '';
  let capturedShortcut: string | undefined;
  let wasOpen = false;

  return {
    view: ({ attrs }) => {
      // Reset transient state each time the dialog is (re-)opened rather than leaking it across
      // sessions, since ModalPanel never unmounts its content between opens.
      if (attrs.open && !wasOpen) {
        query = '';
        capturedShortcut = undefined;
      }
      wasOpen = attrs.open;

      const context = { scope: 'table' as const, platform: attrs.platform, runtime: attrs.runtime };
      const bindings = getLiveBindings(attrs.actions, attrs.keybindings, context)
        .filter((binding) => binding.available)
        .flatMap((binding) => {
          const action = attrs.actions.find((candidate) => candidate.id === binding.actionId);
          return action === undefined ? [] : [{ binding, action }];
        })
        .sort((a, b) => a.action.title.localeCompare(b.action.title));

      const normalizedQuery = query.trim().toLowerCase();
      const filtered =
        normalizedQuery === ''
          ? bindings
          : bindings.filter(
              ({ binding, action }) =>
                action.title.toLowerCase().includes(normalizedQuery) ||
                (action.description ?? '').toLowerCase().includes(normalizedQuery) ||
                binding.shortcut.toLowerCase().includes(normalizedQuery),
            );

      const captureMatches =
        capturedShortcut === undefined
          ? []
          : bindings.filter(({ binding }) => binding.shortcut === capturedShortcut);

      return m(ModalPanel, {
        title: t('keybindingsHelp', 'title'),
        className: 'fm-shortcuts-help-modal',
        description: m('div', [
          m('input.fm-shortcuts-help-search', {
            type: 'text',
            placeholder: t('keybindingsHelp', 'filterPlaceholder'),
            'aria-label': t('keybindingsHelp', 'filterAriaLabel'),
            value: query,
            oninput: (event: InputEvent) => {
              query = (event.currentTarget as HTMLInputElement).value;
            },
          }),
          m('.fm-shortcuts-help-capture', [
            m(
              'label',
              { for: 'fm-shortcuts-help-capture-input' },
              t('keybindingsHelp', 'captureLabel'),
            ),
            m('input#fm-shortcuts-help-capture-input.fm-shortcuts-help-capture-input', {
              type: 'text',
              readonly: true,
              placeholder: t('keybindingsHelp', 'capturePlaceholder'),
              value: capturedShortcut ?? '',
              onkeydown: (event: KeyboardEvent) => {
                if (event.key === 'Escape') {
                  // Clear the capture instead of letting ModalPanel's closeOnEsc close the dialog.
                  event.preventDefault();
                  event.stopPropagation();
                  capturedShortcut = undefined;
                  return;
                }
                if (BARE_MODIFIER_KEYS.has(event.key)) return;
                // Bare Tab (no modifier) must keep moving focus normally, not get trapped here.
                if (event.key === 'Tab' && !event.ctrlKey && !event.metaKey) return;
                event.preventDefault();
                event.stopPropagation();
                capturedShortcut = normalizedShortcut({
                  key: event.key,
                  ctrl: event.ctrlKey || event.metaKey,
                  shift: event.shiftKey,
                  alt: event.altKey,
                });
              },
            }),
            capturedShortcut === undefined
              ? undefined
              : m(
                  '.fm-shortcuts-help-capture-result',
                  captureMatches.length === 0
                    ? t('keybindingsHelp', 'noActionBound', { shortcut: capturedShortcut ?? '' })
                    : captureMatches
                        .map(({ action }) => `${capturedShortcut} → ${action.title}`)
                        .join(', '),
                ),
          ]),
          m(
            'table.fm-shortcuts-help-table',
            m(
              'tbody',
              filtered.map(({ binding, action }) =>
                m('tr', { key: `${action.id}:${binding.shortcut}` }, [
                  m('td', [
                    m('div', action.title),
                    action.description === undefined ? undefined : m('small', action.description),
                  ]),
                  m('td', m('kbd', binding.shortcut)),
                ]),
              ),
            ),
          ),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onClose();
        },
        buttons: [{ label: t('keybindingsHelp', 'close'), onclick: () => attrs.onClose() }],
      });
    },
  };
};
