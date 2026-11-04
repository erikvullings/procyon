import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { FinderTag } from '../../models';
import { FinderTagsEditor } from './finder-tags-editor';

export interface FinderTagsDialogAttrs {
  readonly open: boolean;
  readonly entryName: string;
  readonly initialTags: readonly FinderTag[];
  readonly onConfirm: (tags: readonly FinderTag[]) => void;
  readonly onCancel: () => void;
}

/** Minimal modal for assigning, removing and creating Finder tags on one entry (task 0136). The
 * editing markup lives in the shared {@link FinderTagsEditor}; this dialog is just its modal shell. */
export const FinderTagsDialog: FactoryComponent<FinderTagsDialogAttrs> = () => ({
  view: ({ attrs }) =>
    m(ModalPanel, {
      title: t('entryMetadata', 'tagsTitle', { name: attrs.entryName }),
      className: 'fm-dense-modal',
      description: m(FinderTagsEditor, {
        open: attrs.open,
        initialTags: attrs.initialTags,
        onConfirm: attrs.onConfirm,
        onCancel: attrs.onCancel,
      }),
      isOpen: attrs.open,
      closeOnEsc: true,
      onToggle: (open: boolean) => {
        if (!open) attrs.onCancel();
      },
      buttons: [],
    }),
});
