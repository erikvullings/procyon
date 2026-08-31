import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import { validateDirectoryName } from './create-directory-dialog';

export interface CreateFileDialogAttrs {
  readonly open: boolean;
  readonly onConfirm: (name: string) => void;
  readonly onCancel: () => void;
}

/**
 * Moves focus away from the input before the modal closes, so the browser
 * never has to apply aria-hidden to an ancestor of the focused element.
 */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/**
 * Materialized modal used by the Shift+F4 "new file here" action (Total
 * Commander parity, task 0128). Reuses `validateDirectoryName`'s
 * cross-platform-safe single-segment name check - the same rules that make a
 * name unsafe for a directory make it unsafe for a file.
 */
export const CreateFileDialog: FactoryComponent<CreateFileDialogAttrs> = () => {
  let name = '';
  let error: string | undefined;
  let wasOpen = false;

  function confirm(attrs: CreateFileDialogAttrs): void {
    error = validateDirectoryName(name);
    if (error === undefined) {
      blurActive();
      attrs.onConfirm(name);
    }
  }

  function cancel(attrs: CreateFileDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  return {
    onupdate: ({ attrs }) => {
      // ModalPanel keeps this component (and its input) permanently mounted and
      // only toggles CSS visibility, so a plain oncreate-focus only ever fires
      // once at app boot, before the dialog is ever shown. Focus explicitly on
      // the false->true transition instead.
      if (attrs.open && !wasOpen) {
        name = '';
        error = undefined;
        document.getElementById('create-file-name')?.focus();
      }
      wasOpen = attrs.open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('operation', 'newFile'),
        className: 'fm-dense-modal',
        description: m('label.fm-create-directory-field', [
          m('span', t('operation', 'fileName')),
          m('input#create-file-name', {
            type: 'text',
            value: name,
            required: true,
            'aria-invalid': error === undefined ? undefined : 'true',
            oninput: (event: InputEvent) => {
              name = (event.currentTarget as HTMLInputElement).value;
              error = validateDirectoryName(name);
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
          error === undefined ? undefined : m('.fm-field-error', error),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('button', 'create'),
            disabled: validateDirectoryName(name) !== undefined,
            onclick: () => confirm(attrs),
          },
        ],
      }),
  };
};
