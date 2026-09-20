import m, { type FactoryComponent } from 'mithril';
import { AlertDialog, type ModalCloseReason } from 'mithril-materialized';
import { arrowRightIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { Connection, Location } from '../../models';
import { connectionForLocation } from '../connections/connections-model';
import { parentLocation } from '../navigation/navigation';
import { createDialogFocusCycle } from './dialog-focus';
import type { OperationConfirmationRequest } from './operations-controller';

export interface OperationConfirmationDialogAttrs {
  readonly request?: OperationConfirmationRequest;
  readonly connections: readonly Connection[];
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

function displayLocation(location: Location, connections: readonly Connection[]): string {
  const connection = connectionForLocation(location, connections);
  if (connection === undefined) return displayUri(location.uri);
  try {
    return `${connection.name} · ${decodeURIComponent(new URL(location.uri).pathname)}`;
  } catch {
    return connection.name;
  }
}

function itemCount(count: number): string {
  return t('operation', 'itemsProgress', count);
}

function transferDescription(
  request: OperationConfirmationRequest,
  connections: readonly Connection[],
): m.Children {
  const destination = request.destination;
  if (destination === undefined) return undefined;
  const destinationLabel = displayLocation(destination, connections);
  return m('.fm-operation-confirmation-description', [
    m('.fm-operation-confirmation-route', [
      m('.fm-operation-confirmation-endpoint.fm-operation-confirmation-source', [
        m('span.fm-operation-confirmation-label', t('operation', 'source')),
        m(
          '.fm-operation-confirmation-locations',
          sourceDirectories(request.sources).map((location) => {
            const label = displayLocation(location, connections);
            return m('code', { title: label }, label);
          }),
        ),
      ]),
      m(
        '.fm-operation-confirmation-arrow',
        { 'aria-hidden': 'true' },
        arrowRightIcon({ size: 18 }),
      ),
      m('.fm-operation-confirmation-endpoint', [
        m('span.fm-operation-confirmation-label', t('operation', 'destination')),
        m('code', { title: destinationLabel }, destinationLabel),
      ]),
    ]),
  ]);
}

/** Confirmation before starting a routine copy, move, or Trash operation. */
export const OperationConfirmationDialog: FactoryComponent<
  OperationConfirmationDialogAttrs
> = () => {
  const focusCycle = createDialogFocusCycle('.mm-dialog-primary-action');
  return {
    onremove: focusCycle.unmount,
    view: ({ attrs }) => {
      const request = attrs.request;
      const kind = request?.kind ?? 'copy';
      const title =
        request === undefined
          ? t('operation', kind)
          : t(
              'operation',
              kind === 'trash'
                ? 'confirmTrashSummary'
                : kind === 'move'
                  ? 'confirmMoveSummary'
                  : 'confirmCopySummary',
              {
                items: itemCount(request.sources.length),
              },
            );
      const description =
        request === undefined || kind === 'trash'
          ? undefined
          : transferDescription(request, attrs.connections);
      return m(AlertDialog, {
        className: 'fm-operation-confirmation-modal',
        title,
        description:
          request === undefined
            ? undefined
            : m(
                '.fm-operation-confirmation-focus-scope',
                {
                  oncreate: ({ dom }) => focusCycle.mount(dom),
                  onremove: focusCycle.unmount,
                },
                description,
              ),
        isOpen: request !== undefined,
        closeOnEsc: true,
        closeOnButtonClick: false,
        initialFocus: false,
        trapFocus: false,
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
  };
};
