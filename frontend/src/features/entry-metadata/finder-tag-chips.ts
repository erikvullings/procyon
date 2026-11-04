import m from 'mithril';
import { t } from '../../i18n';
import type { FinderTag } from '../../models';
import { finderTagColorSwatch } from '../directory-table/finder-tag-colors';

/** Renders an entry's Finder tags as the shared chip list used by both the standalone
 * {@link FinderTagsDialog} editor and the Properties dialog summary (task 0145). Pass an
 * `onRemove` callback to make each chip's remove button fire; omit it for a read-only summary. */
export function renderFinderTagChips(
  tags: readonly FinderTag[],
  onRemove?: (index: number) => void,
): m.Vnode {
  return m('ul.fm-finder-tags-list', [
    ...tags.map((tag, index) =>
      m('li.fm-finder-tags-chip', { key: `${tag.name}-${tag.color}-${index}` }, [
        finderTagColorSwatch(tag.color) === undefined
          ? undefined
          : m('span.fm-finder-tag-color-dot', {
              style: { background: finderTagColorSwatch(tag.color) },
            }),
        m('span.fm-finder-tags-chip-name', tag.name),
        onRemove === undefined
          ? undefined
          : m(
              'button.fm-finder-tags-chip-remove',
              {
                type: 'button',
                'aria-label': t('entryMetadata', 'removeTagAriaLabel', { name: tag.name }),
                onclick: () => onRemove(index),
              },
              '×',
            ),
      ]),
    ),
  ]);
}
