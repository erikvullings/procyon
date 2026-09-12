import m, { type FactoryComponent } from 'mithril';
import { AlertDialog, type ModalCloseReason } from 'mithril-materialized';
import { arrowRightIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { Location } from '../../models';
import { parentLocation } from '../navigation/navigation';
import type { OperationConfirmationRequest } from './operations-controller';

export interface OperationConfirmationDialogAttrs {
  readonly request?: OperationConfirmationRequest;
  readonly onConfirm: () => void;
  readonly onCancel: () => void;
}

function sourceDirectories(sources: readonly Location[]): readonly Location[] {
  const unique = new Map<string, Location>();
  for (const source of sources) {
    const directory = parentLocation(source);
    unique.set(`${directory.providerId}\0${directory.uri}`, directory);
  }
  return [...unique.values()];
}

function displayUri(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

function itemCount(count: number): string {
  return t('operation', 'itemsProgress', count);
}

function transferDescription(request: OperationConfirmationRequest): m.Children {
  const kind = request.kind === 'move' ? 'confirmMoveSummary' : 'confirmCopySummary';
  const destination = request.destination;
  if (destination === undefined) return undefined;
  return m('.fm-operation-confirmation-description', [
    m(
      'p.fm-operation-confirmation-summary',
      t('operation', kind, { items: itemCount(request.sources.length) }),
    ),
    m('.fm-operation-confirmation-route', [
      m('.fm-operation-confirmation-endpoint.fm-operation-confirmation-source', [
        m('span.fm-operation-confirmation-label', t('operation', 'source')),
        m(
          '.fm-operation-confirmation-locations',
          sourceDirectories(request.sources).map((location) =>
            m('code', { title: displayUri(location.uri) }, displayUri(location.uri)),
          ),
        ),
      ]),
      m(
        '.fm-operation-confirmation-arrow',
        { 'aria-hidden': 'true' },
        arrowRightIcon({ size: 18 }),
      ),
      m('.fm-operation-confirmation-endpoint', [
        m('span.fm-operation-confirmation-label', t('operation', 'destination')),
        m('code', { title: displayUri(destination.uri) }, displayUri(destination.uri)),
      ]),
    ]),
  ]);
}

/** Confirmation before starting a routine copy, move, or Trash operation. */
export const OperationConfirmationDialog: FactoryComponent<
  OperationConfirmationDialogAttrs
> = () => ({
  view: ({ attrs }) => {
    const request = attrs.request;
    const kind = request?.kind ?? 'copy';
    return m(AlertDialog, {
      className: 'fm-operation-confirmation-modal',
      title: t('operation', 'confirmOperationTitle'),
      description:
        request === undefined
          ? undefined
          : kind === 'trash'
            ? t('operation', 'confirmTrashSummary', {
                items: itemCount(request.sources.length),
              })
            : transferDescription(request),
      isOpen: request !== undefined,
      closeOnEsc: true,
      closeOnButtonClick: false,
      initialFocus: '.mm-dialog-primary-action',
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
