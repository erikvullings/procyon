import m, { type Component } from 'mithril';

import { t } from '../../i18n';
import type {
  InterruptedTransfer,
  OperationId,
  TransferRecoveryClass,
  TransferRecoveryReason,
} from '../../models';

export interface InterruptedTransfersAttrs {
  readonly transfers: readonly InterruptedTransfer[];
  /** Transfer an action is currently running for; its buttons are disabled. */
  readonly busyId?: OperationId | undefined;
  readonly error?: string | undefined;
  /** `resume` continues copies from their proven partials instead of starting over (task 0303). */
  readonly onRestart: (operationId: OperationId, resume?: boolean) => void;
  readonly onDiscard: (operationId: OperationId) => void;
}

const CLASS_KEY = {
  resumable: 'interruptedClassResumable',
  safelyRestartable: 'interruptedClassSafelyRestartable',
  completedUnconfirmed: 'interruptedClassCompletedUnconfirmed',
  needsIntervention: 'interruptedClassNeedsIntervention',
} as const satisfies Record<TransferRecoveryClass, string>;

const REASON_KEY = {
  notStarted: 'interruptedReasonNotStarted',
  partialUnfinished: 'interruptedReasonPartialUnfinished',
  publishedUnconfirmed: 'interruptedReasonPublishedUnconfirmed',
  sourceChanged: 'interruptedReasonSourceChanged',
  sourceMissing: 'interruptedReasonSourceMissing',
  destinationReplaced: 'interruptedReasonDestinationReplaced',
  providerUnavailable: 'interruptedReasonProviderUnavailable',
  journalDamaged: 'interruptedReasonJournalDamaged',
} as const satisfies Record<TransferRecoveryReason, string>;

const KIND_KEY = {
  copy: 'kindCopy',
  move: 'kindMove',
  duplicate: 'kindDuplicate',
} as const;

function displayUri(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

function kindLabel(kind: InterruptedTransfer['kind']): string {
  const key = kind === 'move' || kind === 'duplicate' ? KIND_KEY[kind] : KIND_KEY.copy;
  return t('operation', key);
}

/**
 * The transfers a crash or restart interrupted, each with what is safe to do about it.
 *
 * Nothing here runs by itself: a transfer is only re-run, or forgotten, when the user chooses.
 * A transfer that needs a decision offers "run again anyway", never a bare "run again".
 */
export const InterruptedTransfers: Component<InterruptedTransfersAttrs> = {
  view: ({ attrs }) => {
    if (attrs.transfers.length === 0) return undefined;
    return m(
      'section.fm-interrupted-transfers',
      { 'aria-label': t('operation', 'interruptedHeading') },
      [
        m('h3', t('operation', 'interruptedHeading')),
        m('p.fm-interrupted-intro', t('operation', 'interruptedIntro')),
        attrs.error === undefined
          ? undefined
          : m('p.fm-operation-failure', { role: 'alert' }, attrs.error),
        m(
          'ul.fm-interrupted-list',
          attrs.transfers.map((transfer) => {
            const busy = attrs.busyId === transfer.operationId;
            const unconfirmed = transfer.class === 'completedUnconfirmed';
            const needsDecision = transfer.class === 'needsIntervention';
            return m(
              'li.fm-interrupted-item',
              { key: transfer.operationId, 'data-class': transfer.class },
              [
                m('.fm-interrupted-heading', [
                  m('strong', kindLabel(transfer.kind)),
                  m('span.fm-interrupted-class', t('operation', CLASS_KEY[transfer.class])),
                ]),
                m(
                  'p.fm-interrupted-paths',
                  [
                    transfer.sources.map((source) => displayUri(source.uri)).join(', '),
                    transfer.destination === undefined || transfer.destination === null
                      ? ''
                      : ` → ${displayUri(transfer.destination.uri)}`,
                  ].join(''),
                ),
                m(
                  'p.fm-interrupted-summary',
                  t('operation', 'interruptedSummary', {
                    done: transfer.committedItems + transfer.unconfirmedItems,
                    total: transfer.totalItems,
                    remaining: transfer.remainingItems,
                  }),
                ),
                m('p.fm-interrupted-reason', t('operation', REASON_KEY[transfer.reason])),
                m('.fm-operation-controls', [
                  transfer.class === 'resumable'
                    ? m(
                        'button',
                        {
                          type: 'button',
                          disabled: busy,
                          title: t('operation', 'interruptedResumeHint'),
                          onclick: () => attrs.onRestart(transfer.operationId, true),
                        },
                        t('operation', 'interruptedResume'),
                      )
                    : undefined,
                  m(
                    'button',
                    {
                      type: 'button',
                      class: unconfirmed ? 'fm-interrupted-secondary' : undefined,
                      disabled: busy,
                      title: t('operation', 'interruptedRunAgainHint'),
                      onclick: () => attrs.onRestart(transfer.operationId, false),
                    },
                    needsDecision
                      ? t('operation', 'interruptedRunAgainAnyway')
                      : t('operation', 'interruptedRunAgain'),
                  ),
                  m(
                    'button',
                    {
                      type: 'button',
                      disabled: busy,
                      title: t('operation', 'interruptedDiscardHint'),
                      onclick: () => attrs.onDiscard(transfer.operationId),
                    },
                    unconfirmed
                      ? t('operation', 'interruptedConfirmDone')
                      : t('operation', 'interruptedDiscard'),
                  ),
                ]),
              ],
            );
          }),
        ),
      ],
    );
  },
};
