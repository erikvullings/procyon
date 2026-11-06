import m, { type FactoryComponent } from 'mithril';
import { AlertDialog, type ModalCloseReason } from 'mithril-materialized';
import { t } from '../../i18n';
import type { ConflictPolicy, Connection, Location, TransferConflicts } from '../../models';
import { connectionForLocation } from '../connections/connections-model';
import { parentLocation } from '../navigation/navigation';
import { affectedItems } from './affected-items';
import { createDialogFocusCycle } from './dialog-focus';
import type { OperationConfirmationRequest } from './operations-controller';
import { readVerifyTransfers, writeVerifyTransfers } from './verify-preference';

export interface OperationConfirmationDialogAttrs {
  readonly request?: OperationConfirmationRequest;
  readonly connections: readonly Connection[];
  /** Native home directory, shown as `~` in local paths when known. */
  readonly homeDirectory?: string | undefined;
  /** Proceeds with the conflict policy the user chose (`ask` when nothing already exists). */
  readonly onConfirm: (conflictPolicy: ConflictPolicy) => void;
  readonly onCancel: () => void;
}

/** Existing names listed before collapsing the rest into "and N more". */
const LISTED_CONFLICT_NAMES = 5;

function conflictRows(conflicts: TransferConflicts): m.Children {
  const listed = conflicts.names.slice(0, LISTED_CONFLICT_NAMES);
  const remaining = conflicts.names.length - listed.length;
  return [
    m('dt.fm-operation-confirmation-label', t('operation', 'alreadyExists')),
    m('dd.fm-operation-confirmation-conflicts', [
      m(
        'ul.fm-operation-confirmation-conflict-names',
        listed.map((name) => m('li', m('code', name))),
      ),
      remaining > 0 ? m('p', t('operation', 'alreadyExistsMore', { count: remaining })) : undefined,
      conflicts.complete ? undefined : m('p', t('operation', 'alreadyExistsIncomplete')),
      m('p.fm-conflict-dialog-recommendation', t('operation', 'conflictRecommendation')),
    ]),
  ];
}

function sourceDirectories(sources: readonly Location[]): readonly Location[] {
  const unique = new Map<string, Location>();
  for (const source of sources) {
    const directory = parentLocation(source);
    unique.set(`${directory.providerId}\0${directory.uri}`, directory);
  }
  return [...unique.values()];
}

function decode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** A human path for a location: connection name + path, a plain local path (with `~` for the
 *  home directory), or the decoded URI for anything else. */
export function displayLocation(
  location: Location,
  connections: readonly Connection[],
  homeDirectory?: string,
): { readonly prefix?: string; readonly path: string } {
  const connection = connectionForLocation(location, connections);
  let url: URL | undefined;
  try {
    url = new URL(location.uri);
  } catch {
    url = undefined;
  }
  if (connection !== undefined) {
    return { prefix: connection.name, path: url === undefined ? '/' : decode(url.pathname) };
  }
  if (url?.protocol === 'file:') {
    const path = decode(url.pathname);
    const home = homeDirectory?.replace(/\/+$/, '');
    if (home !== undefined && home !== '' && (path === home || path.startsWith(`${home}/`))) {
      return { path: `~${path.slice(home.length)}` };
    }
    return { path };
  }
  if (url?.pathname.startsWith('/')) {
    return { path: `${decode(url.host)}${decode(url.pathname)}` };
  }
  return { path: decode(location.uri) };
}

/** Renders a path with its parent muted and its final folder emphasised, breaking only at `/`. */
function pathView(
  location: Location,
  connections: readonly Connection[],
  homeDirectory?: string,
): m.Children {
  const { prefix, path } = displayLocation(location, connections, homeDirectory);
  const trimmed = path.length > 1 ? path.replace(/\/+$/, '') : path;
  const cut = trimmed.lastIndexOf('/');
  const parent = cut >= 0 && trimmed.length > 1 ? trimmed.slice(0, cut + 1) : '';
  const leaf = cut >= 0 && trimmed.length > 1 ? trimmed.slice(cut + 1) : trimmed;
  const full = prefix === undefined ? trimmed : `${prefix} · ${trimmed}`;
  const withBreaks = (text: string): m.Children =>
    text
      .split('/')
      .flatMap((part, index, parts) => (index < parts.length - 1 ? [part, '/', m('wbr')] : [part]));
  return m('span.fm-operation-path', { title: full }, [
    prefix === undefined ? undefined : m('span.fm-operation-path-connection', prefix),
    parent === '' ? undefined : m('span.fm-operation-path-parent', withBreaks(parent)),
    m('span.fm-operation-path-leaf', leaf),
  ]);
}

function itemCount(count: number): string {
  return t('operation', 'itemsProgress', count);
}

function transferRows(
  request: OperationConfirmationRequest,
  connections: readonly Connection[],
  homeDirectory?: string,
): m.Children {
  const destination = request.destination;
  if (destination === undefined) return undefined;
  return [
    m('dt.fm-operation-confirmation-label', t('operation', 'source')),
    m(
      'dd.fm-operation-confirmation-source',
      sourceDirectories(request.sources).map((location) =>
        pathView(location, connections, homeDirectory),
      ),
    ),
    m('dt.fm-operation-confirmation-label', t('operation', 'destination')),
    m(
      'dd.fm-operation-confirmation-destination',
      pathView(destination, connections, homeDirectory),
    ),
  ];
}

/** The "verify copied files" choice, remembered between copies (task 0302). */
function verifyOption(): m.Vnode {
  return m(
    'label.fm-operation-confirmation-verify',
    { title: t('operation', 'verifyOptionHint') },
    [
      m('input', {
        type: 'checkbox',
        checked: readVerifyTransfers(),
        onchange: (event: Event) =>
          writeVerifyTransfers((event.currentTarget as HTMLInputElement).checked),
      }),
      m('span', t('operation', 'verifyOption')),
    ],
  );
}

/** Confirmation before starting a routine copy, move, or Trash operation. */
export const OperationConfirmationDialog: FactoryComponent<
  OperationConfirmationDialogAttrs
> = () => {
  // Plain confirmations focus the primary action; with existing items the safe "keep both"
  // choice is focused instead, so Enter never overwrites (same default as the conflict dialog).
  const primaryFocus = createDialogFocusCycle('.mm-dialog-primary-action');
  const recommendedFocus = createDialogFocusCycle('.fm-conflict-recommended');
  const unmount = (): void => {
    primaryFocus.unmount();
    recommendedFocus.unmount();
  };
  return {
    onremove: unmount,
    view: ({ attrs }) => {
      const request = attrs.request;
      const conflicts = request?.kind === 'trash' ? undefined : request?.conflicts;
      const focusCycle = conflicts === undefined ? primaryFocus : recommendedFocus;
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
      const rows =
        request === undefined || kind === 'trash'
          ? undefined
          : transferRows(request, attrs.connections, attrs.homeDirectory);
      return m(AlertDialog, {
        className: 'fm-operation-confirmation-modal',
        title,
        description:
          request === undefined
            ? undefined
            : m(
                '.fm-operation-confirmation-focus-scope',
                {
                  key: conflicts === undefined ? 'plain' : 'conflicts',
                  oncreate: ({ dom }) => focusCycle.mount(dom),
                  onremove: focusCycle.unmount,
                },
                m('dl.fm-operation-confirmation-facts', [
                  affectedItems(request.sources),
                  rows,
                  conflicts === undefined ? undefined : conflictRows(conflicts),
                ]),
                kind === 'copy' ? verifyOption() : undefined,
              ),
        isOpen: request !== undefined,
        closeOnEsc: true,
        closeOnButtonClick: false,
        initialFocus: false,
        trapFocus: false,
        onClose: (reason: ModalCloseReason) => {
          if (reason !== 'programmatic') attrs.onCancel();
        },
        ...(conflicts === undefined
          ? {
              primaryAction: {
                label: t('operation', kind),
                destructive: kind === 'trash',
                onclick: () => attrs.onConfirm('ask'),
              },
            }
          : {
              actions: [
                { label: t('operation', 'decidePerItem'), onclick: () => attrs.onConfirm('ask') },
                { label: t('button', 'skip'), onclick: () => attrs.onConfirm('skip') },
                {
                  label: t('button', 'renameNew'),
                  className: 'fm-conflict-recommended',
                  onclick: () => attrs.onConfirm('renameNew'),
                },
              ],
              primaryAction: {
                label: t('button', 'overwrite'),
                destructive: true,
                onclick: () => attrs.onConfirm('overwrite'),
              },
            }),
        secondaryAction: {
          label: t('button', 'cancel'),
          onclick: attrs.onCancel,
        },
      });
    },
  };
};
