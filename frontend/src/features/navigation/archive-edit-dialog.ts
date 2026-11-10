import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';

export interface ArchiveEditDialogAttrs {
  readonly open: boolean;
  /** File name of the nested archive that holds the unsaved changes. */
  readonly archiveName: string;
  readonly error?: string;
  /** Replaces the archive inside its containing archive. */
  readonly onSave: () => void;
  /** Throws the changes away. */
  readonly onDiscard: () => void;
  /** Leaves the decision for later; the changes stay pending. */
  readonly onLater: () => void;
}

/**
 * Asks whether the changes made inside an archive that is stored in another archive should be
 * written back. Writing replaces the entry in the containing archive and cannot be undone, so
 * it is never done silently.
 */
export const ArchiveEditDialog: FactoryComponent<ArchiveEditDialogAttrs> = () => ({
  view: ({ attrs }) =>
    m(ModalPanel, {
      title: t('archiveEdit', 'title', { name: attrs.archiveName }),
      className: 'fm-dense-modal',
      description: m('div', [
        m('p', t('archiveEdit', 'body', { name: attrs.archiveName })),
        attrs.error === undefined ? undefined : m('.fm-field-error', attrs.error),
      ]),
      isOpen: attrs.open,
      closeOnEsc: true,
      onToggle: (open: boolean) => {
        if (!open) attrs.onLater();
      },
      buttons: [
        { label: t('archiveEdit', 'discard'), onclick: () => attrs.onDiscard() },
        { label: t('archiveEdit', 'later'), onclick: () => attrs.onLater() },
        { label: t('archiveEdit', 'save'), onclick: () => attrs.onSave() },
      ],
    }),
});
