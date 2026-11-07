import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { TerminalApp } from '../../models';

export interface TerminalPickerDialogAttrs {
  readonly open: boolean;
  readonly apps: readonly TerminalApp[];
  readonly onChoose: (app: TerminalApp) => void;
  readonly onCancel: () => void;
}

/** Moves focus out of the modal before it closes, so the browser never applies aria-hidden to an
 * ancestor of the focused element. */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** Lists the configured external terminals for "Open Terminal With..." (task 0127). */
export const TerminalPickerDialog: FactoryComponent<TerminalPickerDialogAttrs> = () => ({
  view: ({ attrs }) =>
    m(ModalPanel, {
      title: t('terminalPicker', 'title'),
      className: 'fm-dense-modal',
      description:
        attrs.apps.length === 0
          ? m('p.fm-terminal-picker__empty', t('terminalPicker', 'empty'))
          : m(
              'ul.fm-terminal-picker',
              { role: 'list' },
              attrs.apps.map((app) =>
                m(
                  'li',
                  { key: app.name },
                  m(
                    'button.fm-terminal-picker__item',
                    {
                      type: 'button',
                      title: app.command,
                      onclick: () => {
                        blurActive();
                        attrs.onChoose(app);
                      },
                    },
                    app.name,
                  ),
                ),
              ),
            ),
      isOpen: attrs.open,
      closeOnEsc: true,
      onToggle: (open: boolean) => {
        if (!open) {
          blurActive();
          attrs.onCancel();
        }
      },
      buttons: [
        {
          label: t('button', 'cancel'),
          onclick: () => {
            blurActive();
            attrs.onCancel();
          },
        },
      ],
    }),
});
