import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';

export interface DeleteWorkspaceDialogAttrs {
  readonly open: boolean;
  readonly workspaceName: string | undefined;
  readonly onConfirm: () => void;
  readonly onCancel: () => void;
}

/** Destructive-deletion confirmation, mirroring `CloseLastTabDialog`'s pattern. */
export const DeleteWorkspaceDialog: FactoryComponent<DeleteWorkspaceDialogAttrs> = () => ({
  view: ({ attrs }) =>
    m(ModalPanel, {
      className: 'fm-dense-modal fm-delete-workspace-modal',
      title: t('workspace', 'deleteTitle'),
      description: m(
        'p',
        t('workspace', 'deleteMessage', {
          name: attrs.workspaceName ?? t('workspace', 'defaultName'),
        }),
      ),
      isOpen: attrs.open,
      closeOnEsc: true,
      onToggle: (open: boolean) => {
        if (!open) attrs.onCancel();
      },
      buttons: [
        {
          label: t('button', 'cancel'),
          onclick: attrs.onCancel,
          className: 'fm-delete-workspace-cancel',
        },
        { label: t('button', 'delete'), onclick: attrs.onConfirm },
      ],
    }),
});
