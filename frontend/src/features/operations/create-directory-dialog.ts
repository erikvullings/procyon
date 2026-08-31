import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';

export interface CreateDirectoryDialogAttrs {
  readonly open: boolean;
  readonly onConfirm: (name: string) => void;
  readonly onCancel: () => void;
}

/** Validates a single cross-platform-safe directory name. */
export function validateDirectoryName(name: string): string | undefined {
  if (name.length === 0) return 'Enter a folder name.';
  if (name === '.' || name === '..' || name.includes('/') || name.includes('\\')) {
    return 'Use a single folder name.';
  }
  if (name.includes('\0') || /[<>:"|?*]/u.test(name)) {
    return 'The name contains invalid characters.';
  }
  const stem = name.split('.')[0]?.trimEnd().toUpperCase();
  if (
    stem !== undefined &&
    (/^(?:CON|PRN|AUX|NUL)$/u.test(stem) || /^(?:COM|LPT)[1-9]$/u.test(stem))
  ) {
    return 'That name is reserved by Windows.';
  }
  return undefined;
}

/**
 * Moves focus away from the input before the modal closes, so the browser
 * never has to apply aria-hidden to an ancestor of the focused element.
 */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** Materialized modal used by the F7 create-directory action. */
export const CreateDirectoryDialog: FactoryComponent<CreateDirectoryDialogAttrs> = () => {
  let name = '';
  let error: string | undefined;
  let wasOpen = false;

  function confirm(attrs: CreateDirectoryDialogAttrs): void {
    error = validateDirectoryName(name);
    if (error === undefined) {
      blurActive();
      attrs.onConfirm(name);
    }
  }

  function cancel(attrs: CreateDirectoryDialogAttrs): void {
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
        document.getElementById('create-directory-name')?.focus();
      }
      wasOpen = attrs.open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('operation', 'newFolder'),
        className: 'fm-dense-modal',
        description: m('label.fm-create-directory-field', [
          m('span', t('operation', 'folderName')),
          m('input#create-directory-name', {
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
