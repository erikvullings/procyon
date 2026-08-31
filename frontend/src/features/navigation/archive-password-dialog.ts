import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';

export interface ArchivePasswordDialogAttrs {
  readonly open: boolean;
  readonly invalid: boolean;
  readonly archiveLabel: string;
  readonly error?: string;
  readonly onConfirm: (password: string) => void;
  readonly onCancel: () => void;
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** Session-only password prompt shown when an archive provider raises a credential challenge. */
export const ArchivePasswordDialog: FactoryComponent<ArchivePasswordDialogAttrs> = () => {
  let password = '';
  let wasOpen = false;
  let wasInvalid = false;

  function confirm(attrs: ArchivePasswordDialogAttrs): void {
    if (password.length === 0) return;
    const submitted = password;
    password = '';
    blurActive();
    attrs.onConfirm(submitted);
  }

  function cancel(attrs: ArchivePasswordDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) {
        password = '';
        document.getElementById('archive-password')?.focus();
      }
      if (attrs.open && attrs.invalid && !wasInvalid) password = '';
      wasOpen = attrs.open;
      wasInvalid = attrs.invalid;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: attrs.invalid
          ? t('archivePassword', 'invalidTitle')
          : t('archivePassword', 'requiredTitle'),
        className: 'fm-dense-modal',
        description: m('label.fm-archive-password-field', [
          m('span', attrs.archiveLabel),
          m('input#archive-password', {
            type: 'password',
            value: password,
            required: true,
            autocomplete: 'current-password',
            oninput: (event: InputEvent) => {
              password = (event.currentTarget as HTMLInputElement).value;
            },
            onkeydown: (event: KeyboardEvent) => {
              if (event.key === 'Escape') {
                event.stopPropagation();
                cancel(attrs);
              } else if (event.key === 'Enter') {
                event.preventDefault();
                event.stopPropagation();
                confirm(attrs);
              }
            },
          }),
          attrs.error === undefined ? undefined : m('.fm-field-error', attrs.error),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('archivePassword', 'unlock'),
            disabled: password.length === 0,
            onclick: () => confirm(attrs),
          },
        ],
      }),
  };
};
