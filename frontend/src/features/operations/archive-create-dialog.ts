import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';

export type ArchiveFormat = 'zip' | 'sevenZip';

export interface ArchiveCreateDialogAttrs {
  readonly open: boolean;
  readonly moveSources: boolean;
  readonly onConfirm: (name: string, format: ArchiveFormat, compressionLevel?: number) => void;
  readonly onCancel: () => void;
}

export function archiveFileName(
  name: string,
  format: ArchiveFormat,
): { readonly value?: string; readonly error?: string } {
  const trimmed = name.trim();
  if (!trimmed) return { error: t('archiveCreate', 'nameEnter') };
  if (trimmed.includes('/') || trimmed.includes('\\') || trimmed.includes('\0')) {
    return { error: t('archiveCreate', 'nameSingle') };
  }
  const extension = format === 'zip' ? '.zip' : '.7z';
  return { value: trimmed.toLowerCase().endsWith(extension) ? trimmed : `${trimmed}${extension}` };
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** Collects the archive filename, format, and ZIP compression level before packing. */
export const ArchiveCreateDialog: FactoryComponent<ArchiveCreateDialogAttrs> = () => {
  let name = 'archive';
  let format: ArchiveFormat = 'zip';
  let compressionLevel = 6;
  let wasOpen = false;

  function confirm(attrs: ArchiveCreateDialogAttrs): void {
    const result = archiveFileName(name, format);
    if (!result.value) return;
    blurActive();
    attrs.onConfirm(result.value, format, format === 'zip' ? compressionLevel : undefined);
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) {
        name = 'archive';
        format = 'zip';
        compressionLevel = 6;
        document.getElementById('archive-create-name')?.focus();
      }
      wasOpen = attrs.open;
    },
    view: ({ attrs }) => {
      const nameResult = archiveFileName(name, format);
      const cancel = () => {
        blurActive();
        attrs.onCancel();
      };
      return m(ModalPanel, {
        title: attrs.moveSources
          ? t('archiveCreate', 'moveTitle')
          : t('archiveCreate', 'createTitle'),
        className: 'fm-dense-modal',
        description: m('.fm-create-directory-field', [
          m('label', [
            m('span', t('archiveCreate', 'nameLabel')),
            m('input#archive-create-name', {
              type: 'text',
              value: name,
              required: true,
              oninput: (event: InputEvent) => {
                name = (event.currentTarget as HTMLInputElement).value;
              },
              onkeydown: (event: KeyboardEvent) => {
                if (event.key === 'Escape') {
                  event.stopPropagation();
                  cancel();
                }
                if (event.key === 'Enter') {
                  event.preventDefault();
                  event.stopPropagation();
                  confirm(attrs);
                }
              },
            }),
          ]),
          m('label', [
            m('span', t('archiveCreate', 'formatLabel')),
            m(
              'select',
              {
                value: format,
                onchange: (event: Event) => {
                  format = (event.currentTarget as HTMLSelectElement).value as ArchiveFormat;
                },
              },
              [
                m('option', { value: 'zip' }, t('archiveCreate', 'formatZip')),
                m('option', { value: 'sevenZip' }, t('archiveCreate', 'format7z')),
              ],
            ),
          ]),
          format === 'zip'
            ? m('label', [
                m('span', t('archiveCreate', 'compressionLabel')),
                m(
                  'select',
                  {
                    value: String(compressionLevel),
                    onchange: (event: Event) => {
                      compressionLevel = Number((event.currentTarget as HTMLSelectElement).value);
                    },
                  },
                  [
                    m('option', { value: '1' }, t('archiveCreate', 'compressionFast')),
                    m('option', { value: '6' }, t('archiveCreate', 'compressionNormal')),
                    m('option', { value: '9' }, t('archiveCreate', 'compressionMaximum')),
                  ],
                ),
              ])
            : m('.fm-field-help', t('archiveCreate', 'compressionHelp7z')),
          nameResult.error === undefined ? undefined : m('.fm-field-error', nameResult.error),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel();
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: cancel },
          {
            label: attrs.moveSources ? t('archiveCreate', 'move') : t('archiveCreate', 'create'),
            disabled: !nameResult.value,
            onclick: () => confirm(attrs),
          },
        ],
      });
    },
  };
};
