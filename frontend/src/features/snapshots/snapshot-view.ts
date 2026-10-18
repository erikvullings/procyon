import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { SnapshotDiffEntry, SnapshotJobPage } from '../../models';
import type { SnapshotPanelState } from './snapshot-state';

export interface SnapshotViewAttrs {
  readonly state: SnapshotPanelState;
  readonly onCancel: () => void;
  readonly onClose: () => void;
}

function phaseLabel(page: SnapshotJobPage | undefined): string {
  switch (page?.phase) {
    case 'loading':
      return t('snapshots', 'phaseLoading');
    case 'hashing':
      return t('snapshots', 'phaseHashing', {
        done: page.filesProcessed,
        total: page.entriesFound,
      });
    case 'writing':
      return t('snapshots', 'phaseWriting');
    case 'done':
      return '';
    default:
      return t('snapshots', 'phaseListing');
  }
}

function kindLabel(entry: SnapshotDiffEntry): string {
  switch (entry.kind) {
    case 'added':
      return t('snapshots', 'added');
    case 'removed':
      return t('snapshots', 'removed');
    case 'changed':
      return t('snapshots', 'changed');
    case 'corrupted':
      return t('snapshots', 'corrupted');
  }
}

function summaryText(page: SnapshotJobPage): string {
  if (page.error != null) return t('snapshots', 'failed', { message: page.error });
  if (page.isCancelled) return t('snapshots', 'cancelled');
  if (page.kind === 'create') {
    return t('snapshots', 'created', {
      entries: page.entriesFound,
      checksums: page.hasChecksums ? t('snapshots', 'withChecksums') : '',
    });
  }
  const counts = page.counts;
  return counts.added + counts.removed + counts.changed + counts.corrupted === 0
    ? t('snapshots', 'identical')
    : t('snapshots', 'summary', { ...counts });
}

/** Progress and results of a snapshot create or compare job (task 0268). */
export const SnapshotView: FactoryComponent<SnapshotViewAttrs> = () => ({
  view: ({ attrs }) => {
    const { state } = attrs;
    const page = state.page;
    const running = page?.isComplete !== true && state.error === undefined;
    return m(
      'section.duplicate-review.snapshot-review',
      { 'aria-label': t('snapshots', 'title') },
      [
        m('header.duplicate-review__header', [
          m('h2', t('snapshots', 'title')),
          m(
            'span.duplicate-review__summary',
            page?.isComplete === true ? summaryText(page) : phaseLabel(page),
          ),
          running
            ? m(
                'button.duplicate-review__cancel',
                { type: 'button', onclick: () => attrs.onCancel() },
                t('snapshots', 'cancel'),
              )
            : undefined,
          m(
            'button.duplicate-review__close',
            {
              type: 'button',
              'aria-label': t('snapshots', 'close'),
              onclick: () => attrs.onClose(),
            },
            t('button', 'close'),
          ),
        ]),
        state.error === undefined
          ? undefined
          : m('p.duplicate-review__error', { role: 'alert' }, state.error),
        page?.kind === 'compare' && page.isComplete && !page.hasChecksums
          ? m('p.duplicate-review__warnings', t('snapshots', 'noChecksums'))
          : undefined,
        m(
          'ul.duplicate-review__groups',
          state.entries.map((entry) =>
            m('li.duplicate-review__group', { key: `${entry.kind}:${entry.path}` }, [
              m('span.music-duplicates__badge', kindLabel(entry)),
              m('span.duplicate-review__path', { title: entry.path }, ` ${entry.path}`),
            ]),
          ),
        ),
      ],
    );
  },
});
