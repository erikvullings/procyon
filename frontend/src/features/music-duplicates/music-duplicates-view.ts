import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import type { MusicDuplicateFile, MusicDuplicateGroup } from '../../models';
import { formatTrackDuration } from '../playlist/playlist-format';
import { type MusicDuplicatesState, totalReclaimableBytes } from './music-duplicates-state';

export interface MusicDuplicatesViewAttrs {
  readonly state: MusicDuplicatesState;
  readonly isLastCopy: (uri: string) => boolean;
  readonly onToggle: (uri: string) => void;
  readonly onSelectSuggested: () => void;
  readonly onClearSelection: () => void;
  readonly onRemoveSelected: () => void;
  readonly onCancel: () => void;
  readonly onClose: () => void;
}

function bytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MB`;
  return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function fileName(file: MusicDuplicateFile): string {
  const last = file.location.uri.split('/').pop() ?? file.location.uri;
  try {
    return decodeURIComponent(last);
  } catch {
    return last;
  }
}

function quality(file: MusicDuplicateFile): string {
  return [
    file.format?.toUpperCase(),
    file.lossless ? t('musicDuplicates', 'lossless') : undefined,
    file.bitrateKbps == null ? undefined : `${file.bitrateKbps} kbps`,
    formatTrackDuration(file.durationSeconds ?? undefined),
    bytes(file.size),
  ]
    .filter((part) => part !== undefined && part !== '')
    .join(' · ');
}

function phaseLabel(state: MusicDuplicatesState): string {
  switch (state.phase) {
    case 'listing':
      return t('musicDuplicates', 'phaseListing');
    case 'hashing':
      return t('musicDuplicates', 'phaseHashing');
    case 'tagging':
      return t('musicDuplicates', 'phaseTagging', {
        done: state.filesProcessed,
        total: state.filesFound,
      });
    case 'grouping':
      return t('musicDuplicates', 'phaseGrouping');
    case 'done':
      return '';
  }
}

function renderGroup(attrs: MusicDuplicatesViewAttrs, group: MusicDuplicateGroup, index: number) {
  const first = group.files[0];
  const heading =
    first?.artist != null && first.title != null
      ? `${first.artist} — ${first.title}`
      : first === undefined
        ? ''
        : fileName(first);
  return m('li.duplicate-review__group', { key: `${index}:${first?.location.uri ?? ''}` }, [
    m('h3.duplicate-review__group-title', [
      m(
        `span.music-duplicates__badge.music-duplicates__badge--${group.confidence}`,
        group.confidence === 'exact'
          ? t('musicDuplicates', 'exact')
          : t('musicDuplicates', 'likely'),
      ),
      ` ${heading}`,
      m('span.duplicate-review__reclaimable', ` · ${bytes(group.reclaimableBytes)}`),
    ]),
    m(
      'ul.duplicate-review__copies',
      group.files.map((file, fileIndex) => {
        const last = attrs.isLastCopy(file.location.uri);
        return m('li', { key: file.location.uri }, [
          m('label', [
            m('input[type=checkbox]', {
              checked: attrs.state.selectedUris.has(file.location.uri),
              disabled: last && !attrs.state.selectedUris.has(file.location.uri),
              onchange: () => attrs.onToggle(file.location.uri),
            }),
            m('span.duplicate-review__path', { title: file.location.uri }, fileName(file)),
            m('span.music-duplicates__quality', quality(file)),
            fileIndex === group.keepIndex
              ? m('span.duplicate-review__keep-hint', t('musicDuplicates', 'keeper'))
              : undefined,
          ]),
        ]);
      }),
    ),
  ]);
}

/** Review panel for duplicate and near-duplicate music (task 0265). */
export const MusicDuplicatesView: FactoryComponent<MusicDuplicatesViewAttrs> = () => ({
  view: ({ attrs }) => {
    const { state } = attrs;
    const running = !state.isComplete;
    return m('section.duplicate-review', { 'aria-label': t('musicDuplicates', 'title') }, [
      m('header.duplicate-review__header', [
        m('h2', t('musicDuplicates', 'title')),
        m(
          'span.duplicate-review__summary',
          state.isCancelled
            ? t('musicDuplicates', 'cancelled')
            : running
              ? phaseLabel(state)
              : t('musicDuplicates', 'summary', {
                  groups: state.groups.length,
                  size: bytes(totalReclaimableBytes(state.groups)),
                }),
        ),
        running
          ? m(
              'button.duplicate-review__cancel',
              { type: 'button', onclick: () => attrs.onCancel() },
              t('musicDuplicates', 'cancel'),
            )
          : undefined,
        m(
          'button.duplicate-review__close',
          {
            type: 'button',
            'aria-label': t('musicDuplicates', 'close'),
            onclick: () => attrs.onClose(),
          },
          t('button', 'close'),
        ),
      ]),
      state.error === undefined
        ? undefined
        : m('p.duplicate-review__error', { role: 'alert' }, state.error),
      state.unreadableTags > 0
        ? m(
            'p.duplicate-review__warnings',
            t('musicDuplicates', 'unreadableTags', state.unreadableTags),
          )
        : undefined,
      state.isComplete && state.groups.length === 0 && !state.isCancelled
        ? m('p.duplicate-review__empty', t('musicDuplicates', 'none'))
        : undefined,
      state.groups.length > 0
        ? m('.music-duplicates__actions', [
            m(
              'button',
              { type: 'button', onclick: () => attrs.onSelectSuggested() },
              t('musicDuplicates', 'selectSuggested'),
            ),
            m(
              'button',
              {
                type: 'button',
                disabled: state.selectedUris.size === 0,
                onclick: () => attrs.onClearSelection(),
              },
              t('musicDuplicates', 'clearSelection'),
            ),
            m(
              'button',
              {
                type: 'button',
                disabled: state.selectedUris.size === 0 || state.removing,
                onclick: () => attrs.onRemoveSelected(),
              },
              t('musicDuplicates', 'removeSelected', { count: state.selectedUris.size }),
            ),
          ])
        : undefined,
      m(
        'ul.duplicate-review__groups',
        state.groups.map((group, index) => renderGroup(attrs, group, index)),
      ),
    ]);
  },
});
