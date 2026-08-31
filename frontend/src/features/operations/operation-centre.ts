import m, { type Component } from 'mithril';

import { t } from '../../i18n';
import type { Operation, OperationId, OperationKind, OperationState } from '../../models';
import type { OperationCentreState } from './operation-state';

export interface OperationCentreAttrs {
  state: OperationCentreState;
  onCancel: (operationId: OperationId) => void;
  onPause: (operationId: OperationId) => void;
  onResume: (operationId: OperationId) => void;
  onUndo?: (operationId: OperationId) => void;
  onDismiss: (operationId: OperationId) => void;
}

function formatBytes(value: number): string {
  if (value < 1_024) return `${value} B`;
  if (value < 1_048_576) return `${(value / 1_024).toFixed(value % 1_024 === 0 ? 0 : 1)} KiB`;
  return `${(value / 1_048_576).toFixed(1)} MiB`;
}

/** Guards against `null` slipping through instead of an omitted optional field. */
function hasValue<T>(value: T | null | undefined): value is T {
  return value !== undefined && value !== null;
}

/** Below this age, a problem-free operation stays invisible rather than flashing a card that's
 * gone before it could be read or cancelled - only operations substantial enough to matter get
 * one. Failures and warnings always show immediately regardless of age. */
const MIN_VISIBLE_DURATION_MS = 2_000;

function isWorthShowing(operation: Operation): boolean {
  if (operation.state === 'failed' || operation.state === 'completedWithWarnings') return true;
  return Date.now() - Date.parse(operation.createdAt) >= MIN_VISIBLE_DURATION_MS;
}

function currentEntryName(operation: Operation): string | undefined {
  const uri = operation.progress.currentEntry?.location.uri;
  if (uri === undefined) return undefined;
  return entryNameFromUri(uri);
}

function entryNameFromUri(uri: string): string {
  const segment = uri.split('/').at(-1);
  return segment === undefined ? uri : decodeURIComponent(segment);
}

function operationTimestamp(operation: Operation): string {
  return operation.completedAt ?? operation.startedAt ?? operation.createdAt;
}

/** Search operations show a running match count instead of the current-entry filename - the
 * filename being scanned right now is rarely the interesting bit (and looks like a bug report
 * of its own when it lands on some unrelated file deep in `node_modules`), whereas "N files
 * found" directly answers "is this working / how many results so far". */
function searchProgressSummary(operation: Operation): string {
  return t('operation', 'filesFoundRunning', operation.progress.completedItems);
}

/** Directory comparisons never transfer bytes, so - like search - they show a running compared
 * count instead of byte/rate progress. */
function compareProgressSummary(operation: Operation): string {
  return t('operation', 'entriesComparedRunning', operation.progress.completedItems);
}

function cancelledResult(operation: Operation): string {
  const { completedItems, totalItems, completedBytes, totalBytes } = operation.progress;
  const items = `${completedItems}${hasValue(totalItems) ? ` / ${totalItems}` : ''}`;
  if (operation.kind === 'search') {
    return operation.result?.message ?? t('operation', 'cancelledSearch', { items });
  }
  if (operation.kind === 'compare') {
    return operation.result?.message ?? t('operation', 'cancelledCompare', { items });
  }
  const bytes = `${formatBytes(completedBytes)}${
    hasValue(totalBytes) ? ` / ${formatBytes(totalBytes)}` : ''
  }`;
  return operation.result?.message ?? t('operation', 'cancelledItems', { items, bytes });
}

function operationKindLabel(kind: OperationKind | null | undefined): string {
  switch (kind) {
    case 'createArchive':
      return t('operation', 'kindCreateArchive');
    case 'moveToArchive':
      return t('operation', 'kindMoveToArchive');
    case 'createDirectory':
      return t('operation', 'kindCreateDirectory');
    case 'createFile':
      return t('operation', 'kindCreateFile');
    case 'rename':
      return t('operation', 'kindRename');
    case 'copy':
      return t('operation', 'kindCopy');
    case 'move':
      return t('operation', 'kindMove');
    case 'duplicate':
      return t('operation', 'kindDuplicate');
    case 'trash':
      return t('operation', 'kindTrash');
    case 'delete':
      return t('operation', 'kindDelete');
    case 'undo':
      return t('operation', 'kindUndo');
    case 'search':
      return t('operation', 'kindSearch');
    case 'compare':
      return t('operation', 'kindCompare');
    default:
      return t('operation', 'kindGeneric');
  }
}

function operationStateLabel(state: OperationState): string {
  switch (state) {
    case 'queued':
      return t('operation', 'stateQueued');
    case 'planning':
      return t('operation', 'statePlanning');
    case 'running':
      return t('operation', 'stateRunning');
    case 'paused':
      return t('operation', 'statePaused');
    case 'waitingForConflictResolution':
      return t('operation', 'stateWaitingForConflict');
    case 'cancelling':
      return t('operation', 'stateCancelling');
    case 'cancelled':
      return t('operation', 'stateCancelled');
    case 'completed':
      return t('operation', 'stateCompleted');
    case 'completedWithWarnings':
      return t('operation', 'stateCompletedWithWarnings');
    case 'failed':
      return t('operation', 'stateFailed');
    case 'interrupted':
      return t('operation', 'stateInterrupted');
  }
}

function button(label: string, action: string, onclick: () => void) {
  return m('button', { type: 'button', 'data-action': action, onclick }, label);
}

/** Compact event-driven queue shown below the workspace panes. */
export const OperationCentre: Component<OperationCentreAttrs> = {
  view: ({ attrs }) => {
    const operations = Object.values(attrs.state.byId)
      .filter((operation): operation is Operation => operation !== undefined)
      // A dedicated modal (PermanentDeleteDialog/ConflictDialog) already prompts for these -
      // duplicating that as a background card here is redundant.
      .filter((operation) => operation.state !== 'waitingForConflictResolution')
      .filter(isWorthShowing)
      .sort((left, right) => left.createdAt.localeCompare(right.createdAt));
    return m(
      '.fm-operation-centre',
      { 'aria-label': t('operation', 'centre') },
      operations.length === 0
        ? m('p.fm-operation-empty', t('operation', 'empty'))
        : operations.map((operation) => {
            const progress = operation.progress;
            const failure = attrs.state.failuresById[operation.id];
            const warnings = operation.errors ?? [];
            const isSearch = operation.kind === 'search';
            const isCompare = operation.kind === 'compare';
            const hidesByteProgress = isSearch || isCompare;
            const terminal =
              operation.state === 'completed' ||
              operation.state === 'completedWithWarnings' ||
              operation.state === 'failed' ||
              operation.state === 'cancelled' ||
              operation.state === 'interrupted';
            return m('article.fm-operation', { 'data-operation-id': operation.id }, [
              m('.fm-operation-summary', [
                m(
                  'strong',
                  `${operationKindLabel(operation.kind)} - ${operationStateLabel(operation.state)}`,
                ),
                operation.queuePosition === undefined
                  ? undefined
                  : m(
                      'span',
                      t('operation', 'queuePosition', { position: operation.queuePosition }),
                    ),
                isSearch && operation.state === 'running'
                  ? m('span', searchProgressSummary(operation))
                  : undefined,
                isCompare && operation.state === 'running'
                  ? m('span', compareProgressSummary(operation))
                  : undefined,
                !terminal && !hidesByteProgress && currentEntryName(operation) !== undefined
                  ? m('span', currentEntryName(operation))
                  : undefined,
                hidesByteProgress
                  ? undefined
                  : m(
                      'span',
                      t('operation', 'itemsProgress', {
                        items: `${progress.completedItems}${hasValue(progress.totalItems) ? ` / ${progress.totalItems}` : ''}`,
                      }),
                    ),
                hidesByteProgress
                  ? undefined
                  : m(
                      'span',
                      `${formatBytes(progress.completedBytes)}${hasValue(progress.totalBytes) ? ` / ${formatBytes(progress.totalBytes)}` : ''}`,
                    ),
                !terminal && hasValue(progress.bytesPerSecond) && !hidesByteProgress
                  ? m('span', `${formatBytes(progress.bytesPerSecond)}/s`)
                  : undefined,
                m(
                  'time.fm-operation-timestamp',
                  { dateTime: operationTimestamp(operation) },
                  new Date(operationTimestamp(operation)).toLocaleString(),
                ),
              ]),
              operation.sources.length === 0
                ? undefined
                : m('details.fm-operation-sources', [
                    m(
                      'summary.fm-operation-source-summary',
                      { 'aria-label': t('operation', 'toggleFileList') },
                      m(
                        'span.fm-operation-source-preview',
                        operation.sources
                          .map((source) => entryNameFromUri(source.location.uri))
                          .join(', '),
                      ),
                    ),
                    m(
                      'ul',
                      operation.sources.map((source) =>
                        m(
                          'li',
                          { title: source.location.uri },
                          entryNameFromUri(source.location.uri),
                        ),
                      ),
                    ),
                  ]),
              !terminal && hasValue(progress.totalBytes)
                ? m('progress', {
                    value: progress.completedBytes,
                    max: Math.max(progress.totalBytes, 1),
                    'aria-label': t('operation', 'progress', {
                      kind: operationKindLabel(operation.kind),
                    }),
                  })
                : undefined,
              terminal &&
              operation.state !== 'completed' &&
              operation.state !== 'completedWithWarnings'
                ? operation.state === 'cancelled'
                  ? m('.fm-operation-result', cancelledResult(operation))
                  : undefined
                : undefined,
              operation.state !== 'completedWithWarnings' || warnings.length === 0
                ? undefined
                : m('.fm-operation-warning', [
                    m('details', [
                      m(
                        'summary',
                        warnings.length === 1
                          ? t('operation', 'showWarning')
                          : t('operation', 'showWarnings'),
                      ),
                      m(
                        'ul',
                        warnings.map((warning) =>
                          m(
                            'li',
                            `${entryNameFromUri(warning.entry.location.uri)}: ${warning.message}`,
                          ),
                        ),
                      ),
                    ]),
                  ]),
              failure === undefined
                ? undefined
                : m('.fm-operation-failure', [
                    m('span', failure.message),
                    m('details', [
                      m('summary', t('button', 'details')),
                      m('pre', JSON.stringify(failure.details ?? { code: failure.code }, null, 2)),
                    ]),
                  ]),
              m('.fm-operation-controls', [
                operation.undo?.available === true
                  ? button(t('button', 'undo'), 'undo', () => attrs.onUndo?.(operation.id))
                  : undefined,
                operation.state === 'queued' ||
                operation.state === 'planning' ||
                operation.state === 'running'
                  ? button(t('button', 'cancel'), 'cancel', () => attrs.onCancel(operation.id))
                  : undefined,
                operation.state === 'running'
                  ? button(t('button', 'pause'), 'pause', () => attrs.onPause(operation.id))
                  : undefined,
                operation.state === 'paused'
                  ? button(t('button', 'resume'), 'resume', () => attrs.onResume(operation.id))
                  : undefined,
                terminal && operation.undo?.available !== true
                  ? button(t('button', 'dismiss'), 'dismiss', () => attrs.onDismiss(operation.id))
                  : undefined,
              ]),
              terminal &&
              operation.kind !== 'undo' &&
              operation.undo?.available === false &&
              operation.undo.reason !== undefined
                ? m('.fm-operation-undo-reason', operation.undo.reason)
                : undefined,
            ]);
          }),
    );
  },
};
