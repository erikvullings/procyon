import m, { type FactoryComponent } from 'mithril';

import { t } from '../../i18n';
import type { DuplicateGroup, Location } from '../../models';

export interface DuplicateReviewViewAttrs {
  readonly groups: readonly DuplicateGroup[];
  readonly isComplete: boolean;
  readonly isCancelled: boolean;
  readonly warningsCount: number;
  readonly selectedUris: ReadonlySet<string>;
  readonly totalReclaimableBytes: number;
  readonly error?: string;
  /** Whether ticking this URI would remove the group's last surviving copy. */
  readonly isLastCopy: (uri: string) => boolean;
  readonly onToggle: (uri: string) => void;
  readonly onDeleteSelected: () => void;
  readonly onCancel: () => void;
  readonly onClose: () => void;
}

function name(location: Location): string {
  const uri = location.uri.endsWith('/') ? location.uri.slice(0, -1) : location.uri;
  const index = uri.lastIndexOf('/');
  return decodeURIComponent(index === -1 ? uri : uri.slice(index + 1));
}

function bytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MB`;
  return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/**
 * Reviews duplicate groups before anything is deleted (spec §35, task 0077).
 *
 * Hardlink clusters are rendered as their own, visually distinct block: their
 * paths are one file, so deleting one reclaims nothing, and the checkbox is
 * annotated accordingly rather than being silently mixed in with real
 * duplicates. Deletion itself is delegated to the application's normal
 * delete-with-confirmation flow.
 */
export const DuplicateReviewView: FactoryComponent<DuplicateReviewViewAttrs> = () => ({
  view: ({ attrs }) => {
    const selectedCount = attrs.selectedUris.size;
    return m('section.duplicate-review', { 'aria-label': t('checksums', 'duplicateFiles') }, [
      m('header.duplicate-review__header', [
        m('h2', t('checksums', 'duplicateFiles')),
        m(
          'span.duplicate-review__summary',
          attrs.isCancelled
            ? t('checksums', 'duplicateCancelled')
            : attrs.isComplete
              ? t('checksums', 'duplicateSummary', {
                  count: attrs.groups.length,
                  size: bytes(attrs.totalReclaimableBytes),
                })
              : t('checksums', 'duplicateScanning'),
        ),
        !attrs.isComplete &&
          m(
            'button.duplicate-review__cancel',
            { type: 'button', onclick: () => attrs.onCancel() },
            t('button', 'cancel'),
          ),
        m(
          'button.duplicate-review__close',
          {
            type: 'button',
            'aria-label': t('checksums', 'closeDuplicateReview'),
            onclick: () => attrs.onClose(),
          },
          t('button', 'close'),
        ),
      ]),

      attrs.error !== undefined && m('p.duplicate-review__error', { role: 'alert' }, attrs.error),

      attrs.warningsCount > 0 &&
        m(
          'p.duplicate-review__warnings',
          t('checksums', 'duplicateUnreadable', { count: attrs.warningsCount }),
        ),

      attrs.isComplete &&
        attrs.groups.length === 0 &&
        !attrs.isCancelled &&
        m('p.duplicate-review__empty', t('checksums', 'noDuplicateFiles')),

      m(
        'ul.duplicate-review__groups',
        attrs.groups.map((group) =>
          m('li.duplicate-review__group', { key: group.fullHash }, [
            m('h3.duplicate-review__group-title', [
              t('checksums', 'duplicateSizeEach', { size: bytes(group.size) }),
              m(
                'span.duplicate-review__reclaimable',
                t('checksums', 'duplicateReclaimable', {
                  size: bytes(group.reclaimableBytes),
                }),
              ),
            ]),

            group.distinctLocations.length > 0 &&
              m(
                'ul.duplicate-review__copies',
                group.distinctLocations.map((location) => {
                  const last = attrs.isLastCopy(location.uri);
                  return m('li', { key: location.uri }, [
                    m('label', [
                      m('input[type=checkbox]', {
                        checked: attrs.selectedUris.has(location.uri),
                        disabled: last,
                        onchange: () => attrs.onToggle(location.uri),
                      }),
                      m('span.duplicate-review__path', { title: location.uri }, name(location)),
                      last &&
                        m(
                          'span.duplicate-review__keep-hint',
                          t('checksums', 'duplicateLastCopyKept'),
                        ),
                    ]),
                  ]);
                }),
              ),

            // Deliberately unkeyed: these sit alongside the unkeyed heading
            // and copy list above, and Mithril rejects a fragment that mixes
            // keyed and unkeyed siblings.
            ...group.hardlinkClusters.map((cluster) =>
              m('div.duplicate-review__hardlinks', [
                m('p.duplicate-review__hardlink-note', t('checksums', 'duplicateHardlinked')),
                m(
                  'ul.duplicate-review__copies',
                  cluster.locations.map((location) =>
                    m('li', { key: location.uri }, [
                      m('label', [
                        m('input[type=checkbox]', {
                          checked: attrs.selectedUris.has(location.uri),
                          disabled: attrs.isLastCopy(location.uri),
                          onchange: () => attrs.onToggle(location.uri),
                        }),
                        m('span.duplicate-review__path', { title: location.uri }, name(location)),
                      ]),
                    ]),
                  ),
                ),
              ]),
            ),
          ]),
        ),
      ),

      m('footer.duplicate-review__footer', [
        m(
          'button.duplicate-review__delete',
          {
            type: 'button',
            disabled: selectedCount === 0,
            onclick: () => attrs.onDeleteSelected(),
          },
          selectedCount === 0
            ? t('checksums', 'deleteSelectedDuplicates')
            : t('checksums', 'deleteSelectedDuplicateCount', { count: selectedCount }),
        ),
      ]),
    ]);
  },
});
