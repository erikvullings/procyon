import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';

/** Confirmation gate for closing a pane's only remaining tab (spec §37). */
export interface CloseLastTabDialogAttrs {
  readonly open: boolean;
  readonly onConfirm: () => void;
  readonly onCancel: () => void;
}

export const CloseLastTabDialog: FactoryComponent<CloseLastTabDialogAttrs> = () => ({
  view: ({ attrs }) =>
    m(ModalPanel, {
      title: t('closeLastTab', 'title'),
      description: m('p', t('closeLastTab', 'message')),
      isOpen: attrs.open,
      closeOnEsc: true,
      onToggle: (open: boolean) => {
        if (!open) attrs.onCancel();
      },
      buttons: [
        {
          label: t('button', 'cancel'),
          onclick: attrs.onCancel,
          className: 'fm-close-last-tab-cancel',
        },
        { label: t('closeLastTab', 'closeTab'), onclick: attrs.onConfirm },
      ],
    }),
});
