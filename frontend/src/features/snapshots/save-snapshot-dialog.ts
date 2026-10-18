import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { Location } from '../../models';
import { validateEntryName } from '../operations/create-directory-dialog';
import { snapshotFileName } from './snapshot-state';

/** What the dialog is about to record, and where the snapshot file will go. */
export interface SaveSnapshotRequest {
  readonly root: Location;
  /** Directory that receives the snapshot file. */
  readonly directory: Location;
  readonly defaultName: string;
}

export interface SaveSnapshotDialogAttrs {
  readonly request: SaveSnapshotRequest | undefined;
  readonly onConfirm: (fileName: string, withChecksums: boolean) => void;
  readonly onCancel: () => void;
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

function decoded(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

/** "Save snapshot…" modal (task 0268): file name and whether to record checksums. */
export const SaveSnapshotDialog: FactoryComponent<SaveSnapshotDialogAttrs> = () => {
  let name = '';
  let withChecksums = true;
  let wasOpen = false;

  const error = (): string | undefined => validateEntryName(snapshotFileName(name));

  function confirm(attrs: SaveSnapshotDialogAttrs): void {
    if (error() !== undefined) return;
    blurActive();
    attrs.onConfirm(snapshotFileName(name), withChecksums);
  }

  function cancel(attrs: SaveSnapshotDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  return {
    view: ({ attrs }) => {
      const open = attrs.request !== undefined;
      if (open && !wasOpen) {
        name = attrs.request?.defaultName ?? '';
        withChecksums = true;
      }
      wasOpen = open;
      return m(ModalPanel, {
        title: t('snapshots', 'saveAction'),
        className: 'fm-dense-modal',
        description: m('.fm-save-snapshot', [
          m('label.fm-create-directory-field', [
            m('span', t('snapshots', 'nameLabel')),
            m('input#save-snapshot-name', {
              type: 'text',
              value: name,
              'aria-invalid': error() === undefined ? undefined : 'true',
              oninput: (event: InputEvent) => {
                name = (event.currentTarget as HTMLInputElement).value;
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
            error() === undefined ? undefined : m('.fm-field-error', error()),
          ]),
          m('label.fm-snapshot-checksums', [
            m('input[type=checkbox]', {
              checked: withChecksums,
              onchange: (event: Event) => {
                withChecksums = (event.currentTarget as HTMLInputElement).checked;
              },
            }),
            m('span', ` ${t('snapshots', 'checksumsLabel')}`),
          ]),
          attrs.request === undefined
            ? undefined
            : m('p', t('snapshots', 'destination', { path: decoded(attrs.request.directory.uri) })),
        ]),
        isOpen: open,
        closeOnEsc: true,
        onToggle: (isOpen: boolean) => {
          if (!isOpen) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('snapshots', 'save'),
            disabled: error() !== undefined,
            onclick: () => confirm(attrs),
          },
        ],
      });
    },
  };
};
