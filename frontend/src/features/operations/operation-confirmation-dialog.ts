import m, { type FactoryComponent } from 'mithril';
import { AlertDialog, type ModalCloseReason } from 'mithril-materialized';
import { t } from '../../i18n';
import type { OperationConfirmationRequest } from './operations-controller';

export interface OperationConfirmationDialogAttrs {
  readonly request?: OperationConfirmationRequest;
  readonly onConfirm: () => void;
  readonly onCancel: () => void;
}

/** Confirmation before starting a routine copy, move, or Trash operation. */
export const OperationConfirmationDialog: FactoryComponent<
  OperationConfirmationDialogAttrs
> = () => ({
  view: ({ attrs }) => {
    const request = attrs.request;
    const kind = request?.kind ?? 'copy';
    const destination = request?.destination?.uri ?? '';
    return m(AlertDialog, {
      className: 'fm-operation-confirmation-modal',
      title: t('operation', 'confirmOperationTitle'),
      description:
        request === undefined
          ? undefined
          : kind === 'trash'
            ? t('operation', 'confirmTrashSummary', { count: request.sources.length })
            : t('operation', kind === 'copy' ? 'confirmCopySummary' : 'confirmMoveSummary', {
                count: request.sources.length,
                destination,
              }),
      isOpen: request !== undefined,
      closeOnEsc: true,
      closeOnButtonClick: false,
      onClose: (reason: ModalCloseReason) => {
        if (reason !== 'programmatic') attrs.onCancel();
      },
      secondaryAction: {
        label: t('button', 'cancel'),
        onclick: attrs.onCancel,
      },
      primaryAction: {
        label: t('operation', kind),
        destructive: kind === 'trash',
        onclick: attrs.onConfirm,
      },
    });
  },
});
